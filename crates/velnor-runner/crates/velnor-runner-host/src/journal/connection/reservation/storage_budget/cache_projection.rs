//! Read-only cache quotas from one admitted reservation.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::HostError;
use crate::journal::connection::storage::read_transaction;

use super::super::super::ConnectionJournal;
use super::super::super::identity::AssignmentIdentity;
use super::super::admission::launch_is_pristine;
use super::plan::StorageCacheProjection;
use super::profile_store::{self, from_sql};
use super::reservation_store;
use super::role::StorageRole;
use super::valid_digest;

const MAX_CACHE_SAMPLE_AGE_MS: u64 = 30_000;

impl ConnectionJournal {
    /// Read an admitted cache allowance without changing its reservation or sample.
    ///
    /// The projection is available only while the exact launch reservation remains
    /// pristine and its latest matching filesystem sample is fresh.
    pub(crate) async fn storage_cache_projection(
        &self,
        identity: &AssignmentIdentity,
        reservation_id: i64,
    ) -> Result<Option<StorageCacheProjection>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Journal);
        }
        read_transaction(&self.journal, async |connection| {
            self.validate_assignment(connection, identity).await?;
            read(
                connection,
                self.connection_id.as_str(),
                self.binding_digest.as_str(),
                &identity.storage_key(),
                reservation_id,
            )
            .await
        })
        .await
    }
}

pub(super) async fn read(
    connection: &turso::Connection,
    connection_id: &str,
    binding_digest: &str,
    assignment_key: &str,
    reservation_id: i64,
) -> Result<Option<StorageCacheProjection>, HostError> {
    let Some(reservation) =
        read_reservation(connection, connection_id, assignment_key, reservation_id).await?
    else {
        return Ok(None);
    };
    if !reservation_is_eligible(connection, connection_id, binding_digest, &reservation).await? {
        return Ok(None);
    }
    let (profile, plan) = load_reservation_plan(connection, reservation_id, &reservation).await?;
    let Some(sample) = read_fresh_sample(connection, reservation_id, &reservation).await? else {
        return Ok(None);
    };
    build_projection(
        connection,
        reservation_id,
        reservation,
        profile,
        plan,
        sample,
    )
    .await
    .map(Some)
}

struct ReservationRecord {
    state: String,
    binding_digest: String,
    engine_id: String,
    docker_root_digest: String,
    intent_id: i64,
    intent_state: String,
    cleanup_proven: bool,
    profile_generation_id: String,
    profile_digest: String,
    instance_binding_digest: String,
    retained_memory_bytes: u64,
}

struct SampleRecord {
    generation_id: String,
    observed_at: u64,
}

async fn read_reservation(
    connection: &turso::Connection,
    connection_id: &str,
    assignment_key: &str,
    reservation_id: i64,
) -> Result<Option<ReservationRecord>, HostError> {
    let mut rows = connection
        .query(
            "SELECT r.state, r.binding_digest, r.engine_id, r.docker_root_digest, r.intent_id, i.state, i.cleanup_proven, s.profile_generation_id, s.profile_digest, s.instance_binding_digest, s.retained_memory_bytes FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id JOIN capacity_reservation_storage AS s ON s.reservation_id = r.id WHERE r.id = ?1 AND r.connection_id = ?2 AND r.assignment_key = ?3 AND i.assignment_key = ?3 AND i.kind = 'launch'",
            (reservation_id, connection_id, assignment_key),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(ReservationRecord {
        state: row.get(0).map_err(|_| HostError::Journal)?,
        binding_digest: row.get(1).map_err(|_| HostError::Journal)?,
        engine_id: row.get(2).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(3).map_err(|_| HostError::Journal)?,
        intent_id: row.get(4).map_err(|_| HostError::Journal)?,
        intent_state: row.get(5).map_err(|_| HostError::Journal)?,
        cleanup_proven: row.get::<i64>(6).map_err(|_| HostError::Journal)? != 0,
        profile_generation_id: row.get(7).map_err(|_| HostError::Journal)?,
        profile_digest: row.get(8).map_err(|_| HostError::Journal)?,
        instance_binding_digest: row.get(9).map_err(|_| HostError::Journal)?,
        retained_memory_bytes: from_sql(row.get(10).map_err(|_| HostError::Journal)?)?,
    }))
}

async fn reservation_is_eligible(
    connection: &turso::Connection,
    connection_id: &str,
    binding_digest: &str,
    reservation: &ReservationRecord,
) -> Result<bool, HostError> {
    Ok(reservation.binding_digest == binding_digest
        && reservation.state == "reserved"
        && reservation.intent_state == "pending"
        && !reservation.cleanup_proven
        && !reservation.engine_id.is_empty()
        && valid_digest(&reservation.docker_root_digest)
        && launch_is_pristine(connection, connection_id, reservation.intent_id).await?)
}

async fn load_reservation_plan(
    connection: &turso::Connection,
    reservation_id: i64,
    reservation: &ReservationRecord,
) -> Result<
    (
        super::profile::StorageProfile,
        super::plan::StorageReservationPlan,
    ),
    HostError,
> {
    let profile = profile_store::load(connection, &reservation.profile_generation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if profile.profile_digest() != reservation.profile_digest {
        return Err(HostError::Journal);
    }
    let plan = reservation_store::load_plan(
        connection,
        reservation_id,
        &profile,
        &reservation.instance_binding_digest,
    )
    .await?;
    profile.validate_plan(&plan)?;
    if reservation_store::plan_memory(&plan)? != reservation.retained_memory_bytes {
        return Err(HostError::Journal);
    }
    let role_kinds =
        reservation_store::validate_role_shape(connection, reservation_id, &profile).await?;
    reservation_store::validate_sample_history(
        connection,
        reservation_id,
        &profile,
        &plan,
        &reservation.engine_id,
        &reservation.docker_root_digest,
        &role_kinds,
    )
    .await?;
    Ok((profile, plan))
}

async fn read_fresh_sample(
    connection: &turso::Connection,
    reservation_id: i64,
    reservation: &ReservationRecord,
) -> Result<Option<SampleRecord>, HostError> {
    let record = read_latest_sample(connection, reservation_id).await?;
    if record.profile_generation_id != reservation.profile_generation_id
        || record.profile_digest != reservation.profile_digest
        || record.engine_id != reservation.engine_id
        || record.docker_root_digest != reservation.docker_root_digest
        || !is_fresh(record.observed_at)?
    {
        return Ok(None);
    }
    Ok(Some(SampleRecord {
        generation_id: record.generation_id,
        observed_at: record.observed_at,
    }))
}

struct LatestSample {
    generation_id: String,
    profile_generation_id: String,
    profile_digest: String,
    engine_id: String,
    docker_root_digest: String,
    observed_at: u64,
}

async fn read_latest_sample(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<LatestSample, HostError> {
    let mut rows = connection
        .query(
            "SELECT sample_generation_id, profile_generation_id, profile_digest, engine_id, docker_root_digest, observed_at_unix_ms FROM capacity_reservation_storage_samples WHERE reservation_id = ?1 ORDER BY observed_at_unix_ms DESC, sample_generation_id DESC LIMIT 1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    Ok(LatestSample {
        generation_id: row.get(0).map_err(|_| HostError::Journal)?,
        profile_generation_id: row.get(1).map_err(|_| HostError::Journal)?,
        profile_digest: row.get(2).map_err(|_| HostError::Journal)?,
        engine_id: row.get(3).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(4).map_err(|_| HostError::Journal)?,
        observed_at: from_sql(row.get(5).map_err(|_| HostError::Journal)?)?,
    })
}

async fn build_projection(
    connection: &turso::Connection,
    reservation_id: i64,
    reservation: ReservationRecord,
    profile: super::profile::StorageProfile,
    plan: super::plan::StorageReservationPlan,
    sample: SampleRecord,
) -> Result<StorageCacheProjection, HostError> {
    let (live, stage, runner_work) = cache_roles(&profile, &plan)?;
    let filesystem_instance_digest = read_cache_pool_digest(
        connection,
        reservation_id,
        &sample.generation_id,
        runner_work.pool_id(),
    )
    .await?;
    if filesystem_instance_digest != runner_work.filesystem_instance_digest() {
        return Err(HostError::Journal);
    }
    StorageCacheProjection::new(
        reservation_id,
        reservation.intent_id,
        reservation.profile_generation_id,
        reservation.profile_digest,
        sample.generation_id,
        sample.observed_at,
        reservation.engine_id,
        reservation.docker_root_digest,
        runner_work.pool_id().to_owned(),
        filesystem_instance_digest,
        live.max_bytes(),
        live.max_inodes(),
        stage.max_bytes(),
        stage.max_inodes(),
    )
}

fn cache_roles<'a>(
    profile: &'a super::profile::StorageProfile,
    plan: &'a super::plan::StorageReservationPlan,
) -> Result<
    (
        &'a super::profile::StorageRoleLimit,
        &'a super::profile::StorageRoleLimit,
        &'a super::plan::StorageRoleCharge,
    ),
    HostError,
> {
    let live = profile
        .roles()
        .iter()
        .find(|limit| limit.role() == StorageRole::CacheLive)
        .filter(|limit| limit.alias_of() == Some(StorageRole::RunnerWork))
        .ok_or(HostError::Journal)?;
    let stage = profile
        .roles()
        .iter()
        .find(|limit| limit.role() == StorageRole::CacheStage)
        .filter(|limit| limit.alias_of() == Some(StorageRole::RunnerWork))
        .ok_or(HostError::Journal)?;
    let runner_work = plan
        .roles()
        .iter()
        .find(|charge| charge.role() == StorageRole::RunnerWork)
        .ok_or(HostError::Journal)?;
    if live.pool_id() == runner_work.pool_id()
        && stage.pool_id() == runner_work.pool_id()
        && live.medium() == runner_work.medium()
        && stage.medium() == runner_work.medium()
    {
        Ok((live, stage, runner_work))
    } else {
        Err(HostError::Journal)
    }
}

async fn read_cache_pool_digest(
    connection: &turso::Connection,
    reservation_id: i64,
    sample_generation_id: &str,
    pool_id: &str,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT filesystem_instance_digest FROM capacity_reservation_storage_pools WHERE reservation_id = ?1 AND sample_generation_id = ?2 AND pool_id = ?3",
            (reservation_id, sample_generation_id, pool_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    row.get(0).map_err(|_| HostError::Journal)
}

fn is_fresh(observed_at_unix_ms: u64) -> Result<bool, HostError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostError::Journal)?
        .as_millis();
    let now = u64::try_from(now).map_err(|_| HostError::Journal)?;
    Ok(now
        .checked_sub(observed_at_unix_ms)
        .is_some_and(|age| age <= MAX_CACHE_SAMPLE_AGE_MS))
}
