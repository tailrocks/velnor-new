use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::launch::bootstrap::{PreJitCleanupPhase, PreJitCleanupPhaseProof};

use super::super::PreJitCleanupClaim;
use super::{claim_eligible, hold_context};

impl ConnectionJournal {
    /// Read the current phase only while the exact claim lease remains current.
    pub(crate) async fn pre_jit_cleanup_phase(
        &self,
        claim: &PreJitCleanupClaim,
    ) -> Result<Option<PreJitCleanupPhase>, HostError> {
        if self.connection_id().as_str() != claim.connection_id() || claim.generation() <= 0 {
            return Err(HostError::Identity);
        }
        let now = super::unix_seconds()?;
        let connection_id = claim.connection_id().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) =
                hold_context(connection, &connection_id, claim.reservation_id()).await?
            else {
                return Ok(None);
            };
            if context.operation_id != claim.operation_id()
                || context.engine_id != claim.engine_id()
                || context.docker_root_digest != claim.docker_root_digest()
                || super::snapshot::snapshot_digest(connection, claim.reservation_id(), &context)
                    .await?
                    != claim.snapshot_sha256()
                || !claim_eligible(connection, &connection_id, claim.reservation_id(), &context)
                    .await?
            {
                return Ok(None);
            }
            let mut rows = connection
                .query(
                    "SELECT progress.phase FROM capacity_reservation_pre_jit_cleanup_progress AS progress JOIN capacity_reservation_pre_jit_cleanup_claims AS claim ON claim.reservation_id = progress.reservation_id AND claim.connection_id = progress.connection_id AND claim.operation_id = progress.operation_id AND claim.engine_id = progress.engine_id AND claim.docker_root_digest = progress.docker_root_digest AND claim.snapshot_sha256 = progress.snapshot_sha256 AND claim.claim_generation = progress.claim_generation WHERE progress.reservation_id = ?1 AND progress.connection_id = ?2 AND progress.operation_id = ?3 AND progress.engine_id = ?4 AND progress.docker_root_digest = ?5 AND progress.snapshot_sha256 = ?6 AND progress.claim_generation = ?7 AND claim.lease_until = ?8 AND claim.lease_until > ?9 AND claim.phase = 'claimed'",
                    (
                        claim.reservation_id(),
                        connection_id.as_str(),
                        claim.operation_id(),
                        claim.engine_id(),
                        claim.docker_root_digest(),
                        claim.snapshot_sha256(),
                        claim.generation(),
                        claim.lease_until(),
                        now,
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let phase = parse_phase(&row.get::<String>(0).map_err(|_| HostError::Journal)?)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                return Err(HostError::Journal);
            }
            Ok(Some(phase))
        })
        .await
    }

    /// Record the cleanup phase that the runtime observed under its active claim.
    pub(crate) async fn record_pre_jit_cleanup_phase(
        &self,
        claim: &PreJitCleanupClaim,
        proof: &PreJitCleanupPhaseProof,
    ) -> Result<(), HostError> {
        if self.connection_id().as_str() != claim.connection_id()
            || proof.connection_id() != claim.connection_id()
            || proof.reservation_id() != claim.reservation_id()
            || proof.operation_id() != claim.operation_id()
            || proof.engine_id() != claim.engine_id()
            || proof.docker_root_digest() != claim.docker_root_digest()
            || proof.snapshot_sha256() != claim.snapshot_sha256()
            || proof.generation() != claim.generation()
            || proof.evidence_sha256().len() != 64
            || !proof
                .evidence_sha256()
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(HostError::Identity);
        }
        let now = super::unix_seconds()?;
        let connection_id = claim.connection_id().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) =
                hold_context(connection, &connection_id, claim.reservation_id()).await?
            else {
                return Err(HostError::Identity);
            };
            if context.operation_id != claim.operation_id()
                || context.engine_id != claim.engine_id()
                || context.docker_root_digest != claim.docker_root_digest()
                || super::snapshot::snapshot_digest(connection, claim.reservation_id(), &context)
                    .await?
                    != claim.snapshot_sha256()
                || !claim_eligible(connection, &connection_id, claim.reservation_id(), &context)
                    .await?
            {
                return Err(HostError::Identity);
            }
            let from = proof.from_phase().as_str();
            let to = proof.to_phase().as_str();
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_pre_jit_cleanup_progress SET phase = ?1, evidence_sha256 = ?2 WHERE reservation_id = ?3 AND connection_id = ?4 AND operation_id = ?5 AND engine_id = ?6 AND docker_root_digest = ?7 AND snapshot_sha256 = ?8 AND claim_generation = ?9 AND phase = ?10 AND EXISTS (SELECT 1 FROM capacity_reservation_pre_jit_cleanup_claims AS claim WHERE claim.reservation_id = ?3 AND claim.connection_id = ?4 AND claim.operation_id = ?5 AND claim.engine_id = ?6 AND claim.docker_root_digest = ?7 AND claim.snapshot_sha256 = ?8 AND claim.claim_generation = ?9 AND claim.lease_until = ?11 AND claim.lease_until > ?12 AND claim.phase = 'claimed')",
                    (
                        to,
                        proof.evidence_sha256(),
                        claim.reservation_id(),
                        connection_id.as_str(),
                        claim.operation_id(),
                        claim.engine_id(),
                        claim.docker_root_digest(),
                        claim.snapshot_sha256(),
                        claim.generation(),
                        from,
                        claim.lease_until(),
                        now,
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Identity);
            }
            Ok(())
        })
        .await
    }
}

fn parse_phase(value: &str) -> Result<PreJitCleanupPhase, HostError> {
    match value {
        "claimed" => Ok(PreJitCleanupPhase::Claimed),
        "bootstrap_absent" => Ok(PreJitCleanupPhase::BootstrapAbsent),
        "initializer_absent" => Ok(PreJitCleanupPhase::InitializerAbsent),
        "storage_absent" => Ok(PreJitCleanupPhase::StorageAbsent),
        "network_absent" => Ok(PreJitCleanupPhase::NetworkAbsent),
        "settled" => Ok(PreJitCleanupPhase::Settled),
        _ => Err(HostError::Journal),
    }
}
