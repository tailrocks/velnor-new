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

    let reservation_state: String = row.get(0).map_err(|_| HostError::Journal)?;
    let stored_binding: String = row.get(1).map_err(|_| HostError::Journal)?;
    let engine_id: String = row.get(2).map_err(|_| HostError::Journal)?;
    let docker_root_digest: String = row.get(3).map_err(|_| HostError::Journal)?;
    let intent_id: i64 = row.get(4).map_err(|_| HostError::Journal)?;
    let intent_state: String = row.get(5).map_err(|_| HostError::Journal)?;
    let cleanup_proven: bool = row.get::<i64>(6).map_err(|_| HostError::Journal)? != 0;
    let profile_generation_id: String = row.get(7).map_err(|_| HostError::Journal)?;
    let stored_profile_digest: String = row.get(8).map_err(|_| HostError::Journal)?;
    let instance_binding_digest: String = row.get(9).map_err(|_| HostError::Journal)?;
    let retained_memory_bytes = from_sql(row.get(10).map_err(|_| HostError::Journal)?)?;

    if stored_binding != binding_digest
        || reservation_state != "reserved"
        || intent_state != "pending"
        || cleanup_proven
        || engine_id.is_empty()
        || !valid_digest(&docker_root_digest)
        || !launch_is_pristine(connection, connection_id, intent_id).await?
    {
        return Ok(None);
    }

    let profile = profile_store::load(connection, &profile_generation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if profile.profile_digest() != stored_profile_digest {
        return Err(HostError::Journal);
    }
    let plan = reservation_store::load_plan(
        connection,
        reservation_id,
        &profile,
        &instance_binding_digest,
    )
    .await?;
    profile.validate_plan(&plan)?;
    if reservation_store::plan_memory(&plan)? != retained_memory_bytes {
        return Err(HostError::Journal);
    }
    let role_kinds =
        reservation_store::validate_role_shape(connection, reservation_id, &profile).await?;
    reservation_store::validate_sample_history(
        connection,
        reservation_id,
        &profile,
        &plan,
        &engine_id,
        &docker_root_digest,
        &role_kinds,
    )
    .await?;

    let mut sample_rows = connection
        .query(
            "SELECT sample_generation_id, profile_generation_id, profile_digest, engine_id, docker_root_digest, observed_at_unix_ms FROM capacity_reservation_storage_samples WHERE reservation_id = ?1 ORDER BY observed_at_unix_ms DESC, sample_generation_id DESC LIMIT 1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let sample = sample_rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let sample_generation_id: String = sample.get(0).map_err(|_| HostError::Journal)?;
    let sample_profile_generation: String = sample.get(1).map_err(|_| HostError::Journal)?;
    let sample_profile_digest: String = sample.get(2).map_err(|_| HostError::Journal)?;
    let sample_engine_id: String = sample.get(3).map_err(|_| HostError::Journal)?;
    let sample_root_digest: String = sample.get(4).map_err(|_| HostError::Journal)?;
    let sample_observed_at = from_sql(sample.get(5).map_err(|_| HostError::Journal)?)?;
    if sample_profile_generation != profile_generation_id
        || sample_profile_digest != stored_profile_digest
        || sample_engine_id != engine_id
        || sample_root_digest != docker_root_digest
        || !is_fresh(sample_observed_at)?
    {
        return Ok(None);
    }

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
    if live.pool_id() != runner_work.pool_id()
        || stage.pool_id() != runner_work.pool_id()
        || live.medium() != runner_work.medium()
        || stage.medium() != runner_work.medium()
    {
        return Err(HostError::Journal);
    }

    let mut pool_rows = connection
        .query(
            "SELECT filesystem_instance_digest FROM capacity_reservation_storage_pools WHERE reservation_id = ?1 AND sample_generation_id = ?2 AND pool_id = ?3",
            (reservation_id, sample_generation_id.as_str(), runner_work.pool_id()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let pool_row = pool_rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    if pool_rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .is_some()
    {
        return Err(HostError::Journal);
    }
    let filesystem_instance_digest: String = pool_row.get(0).map_err(|_| HostError::Journal)?;
    if filesystem_instance_digest != runner_work.filesystem_instance_digest() {
        return Err(HostError::Journal);
    }

    Ok(Some(StorageCacheProjection::new(
        reservation_id,
        intent_id,
        profile_generation_id,
        stored_profile_digest,
        sample_generation_id,
        sample_observed_at,
        engine_id,
        docker_root_digest,
        runner_work.pool_id().to_owned(),
        filesystem_instance_digest,
        live.max_bytes(),
        live.max_inodes(),
        stage.max_bytes(),
        stage.max_inodes(),
    )?))
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
