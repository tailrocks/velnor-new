use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::journal::resource_probe::ProbeSeed;

mod eligibility;
mod inventory;
mod phase;
mod settlement;
mod snapshot;
use eligibility::{claim_eligible, initial_eligible};
pub(crate) use inventory::{
    PreJitBootstrapContainer, PreJitCleanupInventory, PreJitCreateSpec, PreJitInitializerIdentity,
    PreJitNetworkIdentity, PreJitStorageObject,
};
#[cfg(test)]
pub(crate) use settlement::PreJitCleanupSettlementResult;
use snapshot::snapshot_digest;

use super::{HoldContext, hold_context};

const CLAIM_SECONDS: i64 = 45;

/// This lease permits one cleanup after Docker returns a final container-create response before JIT configuration delivery.
#[must_use]
#[derive(Clone)]
pub(crate) struct PreJitCleanupClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    snapshot_sha256: String,
    generation: i64,
    lease_until: i64,
}

/// This capacity probe belongs to the exact pre-JIT cleanup claim.
#[must_use]
pub(crate) struct PreJitCapacityProbeClaim {
    cleanup: PreJitCleanupClaim,
    profile_generation_id: String,
    profile_digest: String,
    seed: ProbeSeed,
}

impl PreJitCapacityProbeClaim {
    pub(in crate::journal) fn new(
        cleanup: PreJitCleanupClaim,
        profile_generation_id: String,
        profile_digest: String,
        seed: ProbeSeed,
    ) -> Self {
        Self {
            cleanup,
            profile_generation_id,
            profile_digest,
            seed,
        }
    }

    pub(crate) fn cleanup_claim(&self) -> &PreJitCleanupClaim {
        &self.cleanup
    }

    pub(crate) fn profile_generation_id(&self) -> &str {
        &self.profile_generation_id
    }

    pub(crate) fn profile_digest(&self) -> &str {
        &self.profile_digest
    }

    pub(crate) fn seed(&self) -> &ProbeSeed {
        &self.seed
    }

    pub(crate) fn connection_id(&self) -> &str {
        self.cleanup.connection_id()
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.cleanup.reservation_id()
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.cleanup.operation_id()
    }

    pub(crate) fn engine_id(&self) -> &str {
        self.cleanup.engine_id()
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        self.cleanup.docker_root_digest()
    }

    pub(crate) fn snapshot_sha256(&self) -> &str {
        self.cleanup.snapshot_sha256()
    }

    pub(crate) const fn generation(&self) -> i64 {
        self.cleanup.generation()
    }

    pub(crate) fn probe_operation_id(&self) -> &str {
        &self.seed.operation_id
    }

    pub(crate) fn probe_operation_name(&self) -> &str {
        &self.seed.operation_name
    }
}

impl PreJitCleanupClaim {
    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn snapshot_sha256(&self) -> &str {
        &self.snapshot_sha256
    }

    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }

    pub(crate) const fn lease_until(&self) -> i64 {
        self.lease_until
    }
}

impl ConnectionJournal {
    /// Prepare one fresh capacity probe after all pre-JIT resources are absent.
    pub(crate) async fn prepare_pre_jit_capacity_probe(
        &self,
        claim: &PreJitCleanupClaim,
        seed: ProbeSeed,
    ) -> Result<PreJitCapacityProbeClaim, HostError> {
        self.journal_ref()
            .prepare_probe_for_pre_jit_cleanup(self, claim, seed)
            .await
    }

    /// Lease exact pre-JIT cleanup only after a direct terminal Docker response.
    pub(crate) async fn claim_pre_jit_cleanup(
        &self,
        reservation_id: i64,
    ) -> Result<Option<PreJitCleanupClaim>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let now = unix_seconds()?;
        let lease_until = now.checked_add(CLAIM_SECONDS).ok_or(HostError::Journal)?;
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(connection, &connection_id, reservation_id).await?
            else {
                return Ok(None);
            };
            let snapshot_sha256 = snapshot_digest(connection, reservation_id, &context).await?;
            let current = read_claim(connection, reservation_id).await?;
            let generation = match current {
                None => {
                    if !initial_eligible(connection, &connection_id, reservation_id, &context)
                        .await?
                    {
                        return Ok(None);
                    }
                    let inserted = connection.execute(
                        "INSERT INTO capacity_reservation_pre_jit_cleanup_claims (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, snapshot_sha256, claim_generation, lease_until, phase) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, 'claimed')",
                        (reservation_id, connection_id.as_str(), context.operation_id, context.engine_id.as_str(), context.docker_root_digest.as_str(), snapshot_sha256.as_str(), lease_until),
                    ).await.map_err(|_| HostError::Journal)?;
                    if inserted != 1 { return Ok(None); }
                    let progress_inserted = connection.execute(
                        "INSERT INTO capacity_reservation_pre_jit_cleanup_progress (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, snapshot_sha256, claim_generation, phase, evidence_sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 'claimed', ?6)",
                        (reservation_id, connection_id.as_str(), context.operation_id, context.engine_id.as_str(), context.docker_root_digest.as_str(), snapshot_sha256.as_str()),
                    ).await.map_err(|_| HostError::Journal)?;
                    if progress_inserted != 1 { return Err(HostError::Journal); }
                    1
                }
                Some((old_generation, old_lease, old_snapshot, old_phase)) => {
                    if old_lease > now || old_snapshot != snapshot_sha256 || old_phase == "settled" {
                        return Ok(None);
                    }
                    if !claim_eligible(connection, &connection_id, reservation_id, &context)
                        .await?
                    {
                        return Ok(None);
                    }
                    let generation = old_generation.checked_add(1).ok_or(HostError::Journal)?;
                    let changed = connection.execute(
                        "UPDATE capacity_reservation_pre_jit_cleanup_claims SET claim_generation = ?1, lease_until = ?2 WHERE reservation_id = ?3 AND connection_id = ?4 AND operation_id = ?5 AND engine_id = ?6 AND docker_root_digest = ?7 AND snapshot_sha256 = ?8 AND claim_generation = ?9 AND lease_until <= ?10 AND phase = 'claimed'",
                        (generation, lease_until, reservation_id, connection_id.as_str(), context.operation_id, context.engine_id.as_str(), context.docker_root_digest.as_str(), snapshot_sha256.as_str(), old_generation, now),
                    ).await.map_err(|_| HostError::Journal)?;
                    if changed != 1 { return Ok(None); }
                    let progress_changed = connection.execute(
                        "UPDATE capacity_reservation_pre_jit_cleanup_progress SET claim_generation = ?1 WHERE reservation_id = ?2 AND connection_id = ?3 AND operation_id = ?4 AND engine_id = ?5 AND docker_root_digest = ?6 AND snapshot_sha256 = ?7 AND claim_generation = ?8 AND phase != 'settled'",
                        (generation, reservation_id, connection_id.as_str(), context.operation_id, context.engine_id.as_str(), context.docker_root_digest.as_str(), snapshot_sha256.as_str(), old_generation),
                    ).await.map_err(|_| HostError::Journal)?;
                    if progress_changed != 1 { return Err(HostError::Journal); }
                    generation
                }
            };
            Ok(Some(PreJitCleanupClaim {
                connection_id: connection_id.clone(),
                reservation_id,
                operation_id: context.operation_id,
                engine_id: context.engine_id,
                docker_root_digest: context.docker_root_digest,
                snapshot_sha256,
                generation,
                lease_until,
            }))
        })
        .await
    }

    /// Make sure the lease and original object snapshot remain current before effects.
    pub(crate) async fn validate_pre_jit_cleanup_claim(
        &self,
        claim: &PreJitCleanupClaim,
    ) -> Result<bool, HostError> {
        if self.connection_id().as_str() != claim.connection_id
            || claim.reservation_id <= 0
            || claim.generation <= 0
        {
            return Err(HostError::Identity);
        }
        let now = unix_seconds()?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(
                connection,
                &claim.connection_id,
                claim.reservation_id,
            )
            .await?
            else {
                return Ok(false);
            };
            if !claim_eligible(
                connection,
                &claim.connection_id,
                claim.reservation_id,
                &context,
            )
            .await?
            {
                return Ok(false);
            }
            if context.operation_id != claim.operation_id
                || context.engine_id != claim.engine_id
                || context.docker_root_digest != claim.docker_root_digest
                || snapshot_digest(connection, claim.reservation_id, &context).await?
                    != claim.snapshot_sha256
            {
                return Ok(false);
            }
            let mut rows = connection.query(
                "SELECT 1 FROM capacity_reservation_pre_jit_cleanup_claims AS claim JOIN capacity_reservation_pre_jit_cleanup_progress AS progress ON progress.reservation_id = claim.reservation_id AND progress.connection_id = claim.connection_id AND progress.operation_id = claim.operation_id AND progress.engine_id = claim.engine_id AND progress.docker_root_digest = claim.docker_root_digest AND progress.snapshot_sha256 = claim.snapshot_sha256 AND progress.claim_generation = claim.claim_generation WHERE claim.reservation_id = ?1 AND claim.connection_id = ?2 AND claim.operation_id = ?3 AND claim.engine_id = ?4 AND claim.docker_root_digest = ?5 AND claim.snapshot_sha256 = ?6 AND claim.claim_generation = ?7 AND claim.lease_until = ?8 AND claim.lease_until > ?9 AND claim.phase = 'claimed' AND progress.phase != 'settled'",
                (claim.reservation_id, claim.connection_id.as_str(), claim.operation_id, claim.engine_id.as_str(), claim.docker_root_digest.as_str(), claim.snapshot_sha256.as_str(), claim.generation, claim.lease_until, now),
            ).await.map_err(|_| HostError::Journal)?;
            Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
        }).await
    }
}

async fn read_claim(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Option<(i64, i64, String, String)>, HostError> {
    let mut rows = connection.query(
        "SELECT claim.claim_generation, claim.lease_until, claim.snapshot_sha256, progress.phase FROM capacity_reservation_pre_jit_cleanup_claims AS claim JOIN capacity_reservation_pre_jit_cleanup_progress AS progress ON progress.reservation_id = claim.reservation_id WHERE claim.reservation_id = ?1",
        [reservation_id],
    ).await.map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let value = (
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
        row.get(2).map_err(|_| HostError::Journal)?,
        row.get(3).map_err(|_| HostError::Journal)?,
    );
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(value))
}

fn unix_seconds() -> Result<i64, HostError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostError::Journal)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| HostError::Journal)
}
