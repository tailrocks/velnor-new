use std::time::Instant;

use crate::{
    error::HostError,
    journal::{
        StorageAdmissionSample,
        connection::{ConnectionJournal, storage::in_transaction},
    },
    launch::bootstrap::{PreJitCleanupPhase, PreJitCleanupSettlementProof},
};

use super::super::{PreJitCleanupClaim, hold_context};
use super::claim_eligible;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreJitCleanupSettlementResult {
    HeldAtCapacity,
    CapacityAvailable,
}

impl ConnectionJournal {
    /// Record a fresh capacity sample after the pre-JIT cleanup proof.
    pub(crate) async fn record_pre_jit_cleanup_settlement(
        &self,
        claim: &PreJitCleanupClaim,
        proof: &PreJitCleanupSettlementProof,
    ) -> Result<PreJitCleanupSettlementResult, HostError> {
        if self.connection_id().as_str() != claim.connection_id()
            || proof.connection_id() != claim.connection_id()
            || proof.reservation_id() != claim.reservation_id()
            || proof.operation_id() != claim.operation_id()
            || proof.engine_id() != claim.engine_id()
            || proof.docker_root_digest() != claim.docker_root_digest()
            || proof.snapshot_sha256() != claim.snapshot_sha256()
            || proof.generation() != claim.generation()
            || proof.sample_evidence_sha256().len() != 64
            || !is_lower_digest(proof.sample_evidence_sha256())
        {
            return Err(HostError::Identity);
        }
        let connection_id = claim.connection_id().to_owned();
        let now = super::unix_seconds()?;
        let sample = proof.sample();
        let sample_identity = sample.identity();
        if sample_identity.sample_generation_id() != proof.probe_operation_id()
            || sample_identity.profile_generation_id() != proof.profile_generation_id()
            || sample_identity.profile_digest() != proof.profile_digest()
            || sample_identity.engine_id() != claim.engine_id()
            || sample_identity.docker_root_digest() != claim.docker_root_digest()
            || proof.probe_operation_name()
                != format!("velnor-resource-probe-{}", proof.probe_operation_id())
            || proof.probe_container_id().is_empty()
        {
            return Err(HostError::Identity);
        }

        in_transaction(self.journal_ref(), async |connection| {
            record_in_transaction(self, connection, claim, proof, &connection_id, now).await
        })
        .await
    }

    #[cfg(test)]
    pub(crate) async fn pre_jit_cleanup_settlement_outcome(
        &self,
        reservation_id: i64,
    ) -> Result<Option<PreJitCleanupSettlementResult>, HostError> {
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT settlement.outcome FROM capacity_reservation_pre_jit_cleanup_claims AS claim JOIN capacity_reservation_pre_jit_cleanup_progress AS progress ON progress.reservation_id = claim.reservation_id AND progress.connection_id = claim.connection_id AND progress.operation_id = claim.operation_id AND progress.engine_id = claim.engine_id AND progress.docker_root_digest = claim.docker_root_digest AND progress.snapshot_sha256 = claim.snapshot_sha256 AND progress.claim_generation = claim.claim_generation JOIN capacity_reservation_pre_jit_cleanup_settlements AS settlement ON settlement.reservation_id = claim.reservation_id AND settlement.claim_generation = claim.claim_generation AND settlement.connection_id = claim.connection_id AND settlement.operation_id = claim.operation_id AND settlement.engine_id = claim.engine_id AND settlement.docker_root_digest = claim.docker_root_digest AND settlement.snapshot_sha256 = claim.snapshot_sha256 JOIN capacity_reservations AS reservation ON reservation.id = claim.reservation_id WHERE claim.reservation_id = ?1 AND claim.connection_id = ?2 AND reservation.connection_id = claim.connection_id AND progress.phase = 'network_absent' AND claim.phase = 'claimed' AND claim.lease_until > CAST(strftime('%s', 'now') AS INTEGER)",
                    (reservation_id, self.connection_id().as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let outcome = match row.get::<String>(0).map_err(|_| HostError::Journal)?.as_str() {
                "held_at_capacity" => PreJitCleanupSettlementResult::HeldAtCapacity,
                "capacity_available" => PreJitCleanupSettlementResult::CapacityAvailable,
                _ => return Err(HostError::Journal),
            };
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                return Err(HostError::Journal);
            }
            Ok(Some(outcome))
        })
        .await
    }
}

async fn record_in_transaction(
    scope: &ConnectionJournal,
    connection: &turso::Connection,
    claim: &PreJitCleanupClaim,
    proof: &PreJitCleanupSettlementProof,
    connection_id: &str,
    now: i64,
) -> Result<PreJitCleanupSettlementResult, HostError> {
    validate_live_claim(scope, connection, claim, proof, connection_id, now).await?;
    let sample = proof.sample();
    let fits = validate_sample(connection, claim, proof, sample).await?;
    insert_settlement(connection, claim, proof, connection_id, sample, fits).await?;
    Ok(if fits {
        PreJitCleanupSettlementResult::CapacityAvailable
    } else {
        PreJitCleanupSettlementResult::HeldAtCapacity
    })
}

async fn validate_live_claim(
    scope: &ConnectionJournal,
    connection: &turso::Connection,
    claim: &PreJitCleanupClaim,
    proof: &PreJitCleanupSettlementProof,
    connection_id: &str,
    now: i64,
) -> Result<(), HostError> {
    scope.validate_scope(connection).await?;
    let Some(context) = hold_context(connection, connection_id, claim.reservation_id()).await?
    else {
        return Err(HostError::Identity);
    };
    if context.operation_id != claim.operation_id()
        || context.engine_id != claim.engine_id()
        || context.docker_root_digest != claim.docker_root_digest()
        || super::snapshot::snapshot_digest(connection, claim.reservation_id(), &context).await?
            != claim.snapshot_sha256()
        || !claim_eligible(connection, connection_id, claim.reservation_id(), &context).await?
    {
        return Err(HostError::Identity);
    }
    validate_claim_lease_and_probe(connection, claim, proof, now).await?;
    let mut phases = connection
        .query(
            "SELECT phase FROM capacity_reservation_pre_jit_cleanup_progress WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?3 AND engine_id = ?4 AND docker_root_digest = ?5 AND snapshot_sha256 = ?6 AND claim_generation = ?7",
            (claim.reservation_id(), connection_id, claim.operation_id(), claim.engine_id(), claim.docker_root_digest(), claim.snapshot_sha256(), claim.generation()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let phase = phases
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Identity)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    if phases
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .is_some()
        || phase != PreJitCleanupPhase::NetworkAbsent.as_str()
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn validate_sample(
    connection: &turso::Connection,
    claim: &PreJitCleanupClaim,
    proof: &PreJitCleanupSettlementProof,
    sample: &StorageAdmissionSample,
) -> Result<bool, HostError> {
    let profile = super::super::super::storage_budget::current_profile(connection)
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    if profile.generation_id() != proof.profile_generation_id()
        || profile.profile_digest() != proof.profile_digest()
    {
        return Err(HostError::Identity);
    }
    sample.validate_metadata_settlement_for(
        &profile,
        proof.probe_operation_id(),
        claim.engine_id(),
        claim.docker_root_digest(),
        Instant::now(),
    )?;
    if sample
        .observed_at()
        .checked_duration_since(Instant::now())
        .is_some()
    {
        return Err(HostError::Identity);
    }
    validate_reserved_pool_identities(connection, claim.reservation_id(), sample).await?;
    super::super::admission::metadata_capacity_fits(
        connection,
        claim.reservation_id(),
        &profile,
        sample,
    )
    .await
}

async fn insert_settlement(
    connection: &turso::Connection,
    claim: &PreJitCleanupClaim,
    proof: &PreJitCleanupSettlementProof,
    connection_id: &str,
    sample: &StorageAdmissionSample,
    fits: bool,
) -> Result<(), HostError> {
    let outcome = if fits {
        "capacity_available"
    } else {
        "held_at_capacity"
    };
    connection
        .execute(
            "INSERT INTO capacity_reservation_pre_jit_cleanup_settlements (reservation_id, claim_generation, connection_id, operation_id, engine_id, docker_root_digest, snapshot_sha256, probe_operation_id, probe_operation_name, probe_container_id, profile_generation_id, profile_digest, observed_at_unix_ms, sample_evidence_sha256, outcome) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            (claim.reservation_id(), claim.generation(), connection_id, claim.operation_id(), claim.engine_id(), claim.docker_root_digest(), claim.snapshot_sha256(), proof.probe_operation_id(), proof.probe_operation_name(), proof.probe_container_id(), proof.profile_generation_id(), proof.profile_digest(), to_sql(sample.observed_at_unix_ms())?, proof.sample_evidence_sha256(), outcome),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    insert_sampled_pools(connection, claim, sample).await
}

async fn insert_sampled_pools(
    connection: &turso::Connection,
    claim: &PreJitCleanupClaim,
    sample: &StorageAdmissionSample,
) -> Result<(), HostError> {
    for pool in sample
        .pools()
        .iter()
        .filter(|pool| pool.medium() == crate::journal::StorageMedium::Disk)
    {
        connection
            .execute(
                "INSERT INTO capacity_reservation_pre_jit_cleanup_settlement_pools (reservation_id, claim_generation, pool_id, filesystem_instance_digest, medium, filesystem_kind, total_bytes, available_bytes, total_inodes, available_inodes) VALUES (?1, ?2, ?3, ?4, 'disk', ?5, ?6, ?7, ?8, ?9)",
                (claim.reservation_id(), claim.generation(), pool.pool_id(), pool.filesystem_instance_digest(), pool.filesystem_kind().as_str(), to_sql(pool.total_bytes())?, to_sql(pool.available_bytes())?, to_sql(pool.total_inodes())?, to_sql(pool.available_inodes())?),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn validate_claim_lease_and_probe(
    connection: &turso::Connection,
    claim: &PreJitCleanupClaim,
    proof: &PreJitCleanupSettlementProof,
    now: i64,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT probe.phase, probe.probe_purpose, probe.engine_id, probe.docker_root_digest, probe.operation_name, probe.container_id, lease.state, lease.engine_id, lease.docker_root_digest, lease.operation_name, lease.container_id FROM capacity_reservation_pre_jit_cleanup_claims AS claim JOIN capacity_reservation_pre_jit_cleanup_progress AS progress ON progress.reservation_id = claim.reservation_id AND progress.connection_id = claim.connection_id AND progress.operation_id = claim.operation_id AND progress.engine_id = claim.engine_id AND progress.docker_root_digest = claim.docker_root_digest AND progress.snapshot_sha256 = claim.snapshot_sha256 AND progress.claim_generation = claim.claim_generation JOIN resource_probe_operations AS probe ON probe.operation_id = ?8 JOIN host_operation_leases AS lease ON lease.singleton = 1 AND lease.operation_id = probe.operation_id JOIN capacity_reservation_pre_jit_cleanup_probes AS prepared ON prepared.probe_operation_id = probe.operation_id AND prepared.reservation_id = claim.reservation_id AND prepared.claim_generation = claim.claim_generation AND prepared.connection_id = claim.connection_id AND prepared.operation_id = claim.operation_id AND prepared.engine_id = claim.engine_id AND prepared.docker_root_digest = claim.docker_root_digest AND prepared.snapshot_sha256 = claim.snapshot_sha256 WHERE claim.reservation_id = ?1 AND claim.connection_id = ?2 AND claim.operation_id = ?3 AND claim.engine_id = ?4 AND claim.docker_root_digest = ?5 AND claim.snapshot_sha256 = ?6 AND claim.claim_generation = ?7 AND claim.phase = 'claimed' AND claim.lease_until > ?9 AND progress.phase = 'network_absent'",
            (claim.reservation_id(), claim.connection_id(), claim.operation_id(), claim.engine_id(), claim.docker_root_digest(), claim.snapshot_sha256(), claim.generation(), proof.probe_operation_id(), now),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Identity)?;
    let phase: String = row.get(0).map_err(|_| HostError::Journal)?;
    let purpose: String = row.get(1).map_err(|_| HostError::Journal)?;
    let engine: String = row.get(2).map_err(|_| HostError::Journal)?;
    let root: String = row.get(3).map_err(|_| HostError::Journal)?;
    let name: String = row.get(4).map_err(|_| HostError::Journal)?;
    let container: Option<String> = row.get(5).map_err(|_| HostError::Journal)?;
    let lease_state: String = row.get(6).map_err(|_| HostError::Journal)?;
    let lease_engine: String = row.get(7).map_err(|_| HostError::Journal)?;
    let lease_root: String = row.get(8).map_err(|_| HostError::Journal)?;
    let lease_name: String = row.get(9).map_err(|_| HostError::Journal)?;
    let lease_container: Option<String> = row.get(10).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || phase != "removed"
        || purpose != "capacity"
        || engine != claim.engine_id()
        || root != claim.docker_root_digest()
        || name != proof.probe_operation_name()
        || container.as_deref() != Some(proof.probe_container_id())
        || lease_state != "released"
        || lease_engine != engine
        || lease_root != root
        || lease_name != name
        || lease_container != container
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn validate_reserved_pool_identities(
    connection: &turso::Connection,
    reservation_id: i64,
    sample: &StorageAdmissionSample,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT pool_id, filesystem_instance_digest FROM capacity_reservation_provisioning_pools WHERE reservation_id = ?1 AND scope = 'shared-disk' AND medium = 'disk' ORDER BY pool_id",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut observed = sample
        .pools()
        .iter()
        .filter(|pool| pool.medium() == crate::journal::StorageMedium::Disk)
        .collect::<Vec<_>>();
    observed.sort_by(|left, right| left.pool_id().cmp(right.pool_id()));
    let mut count = 0_usize;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let id: String = row.get(0).map_err(|_| HostError::Journal)?;
        let digest: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
        let Some(pool) = observed.get(count) else {
            return Err(HostError::Identity);
        };
        if pool.pool_id() != id || digest.as_deref() != Some(pool.filesystem_instance_digest()) {
            return Err(HostError::Identity);
        }
        count += 1;
    }
    if count == 0 || count != observed.len() {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn to_sql(value: u64) -> Result<i64, HostError> {
    i64::try_from(value).map_err(|_| HostError::Journal)
}

fn is_lower_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
