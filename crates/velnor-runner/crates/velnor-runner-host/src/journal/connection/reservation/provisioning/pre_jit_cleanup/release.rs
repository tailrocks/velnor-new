use crate::error::HostError;
use crate::journal::connection::{ConnectionJournal, storage::in_transaction};

use super::{PreJitCleanupClaim, claim_eligible, hold_context};

impl ConnectionJournal {
    /// This method releases a pre-JIT attempt only after cleanup is complete and a fresh capacity sample passes.
    pub(crate) async fn release_pre_jit_attempt(
        &self,
        claim: &PreJitCleanupClaim,
    ) -> Result<bool, HostError> {
        if self.connection_id().as_str() != claim.connection_id() {
            return Err(HostError::Identity);
        }
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            release_claimed_attempt(self, connection, claim).await
        })
        .await
    }
}

async fn release_claimed_attempt(
    scope: &ConnectionJournal,
    connection: &turso::Connection,
    claim: &PreJitCleanupClaim,
) -> Result<bool, HostError> {
    let connection_id = claim.connection_id().to_owned();
    let Some(context) = hold_context(connection, &connection_id, claim.reservation_id()).await?
    else {
        return Ok(false);
    };
    if context.operation_id != claim.operation_id()
        || context.engine_id != claim.engine_id()
        || context.docker_root_digest != claim.docker_root_digest()
        || super::snapshot::snapshot_digest(connection, claim.reservation_id(), &context).await?
            != claim.snapshot_sha256()
        || !claim_eligible(connection, &connection_id, claim.reservation_id(), &context).await?
    {
        return Ok(false);
    }
    let Some((lineage_id, generation, digest, network_name, network_id)) =
        capacity_available_settlement(connection, scope, claim).await?
    else {
        return Ok(false);
    };
    insert_terminal_outcome(
        connection,
        lineage_id,
        generation,
        claim.reservation_id(),
        &digest,
    )
    .await?;
    release_network_row(connection, claim, &network_name, network_id.as_deref()).await?;
    release_reservation(connection, claim).await?;
    remove_current_attempt(connection, lineage_id, generation, claim).await?;
    Ok(true)
}

async fn release_network_row(
    connection: &turso::Connection,
    claim: &PreJitCleanupClaim,
    network_name: &str,
    network_id: Option<&str>,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE capacity_reservation_networks SET remove_state = 'removed' WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?3 AND engine_id = ?4 AND docker_root_digest = ?5 AND network_name = ?6 AND network_id IS ?7 AND create_state = 'created' AND remove_state = 'not_requested' AND EXISTS (SELECT 1 FROM capacity_reservation_pre_jit_cleanup_claims AS claim JOIN capacity_reservation_pre_jit_cleanup_progress AS progress ON progress.reservation_id = claim.reservation_id AND progress.claim_generation = claim.claim_generation JOIN capacity_reservation_pre_jit_cleanup_settlements AS settlement ON settlement.reservation_id = claim.reservation_id AND settlement.claim_generation = claim.claim_generation WHERE claim.reservation_id = ?1 AND claim.connection_id = ?2 AND claim.operation_id = ?3 AND claim.engine_id = ?4 AND claim.docker_root_digest = ?5 AND claim.snapshot_sha256 = ?8 AND claim.claim_generation = ?9 AND claim.lease_until = ?10 AND claim.lease_until > CAST(strftime('%s', 'now') AS INTEGER) AND claim.phase = 'claimed' AND progress.connection_id = claim.connection_id AND progress.operation_id = claim.operation_id AND progress.engine_id = claim.engine_id AND progress.docker_root_digest = claim.docker_root_digest AND progress.snapshot_sha256 = claim.snapshot_sha256 AND progress.claim_generation = claim.claim_generation AND progress.phase = 'network_absent' AND settlement.connection_id = claim.connection_id AND settlement.operation_id = claim.operation_id AND settlement.engine_id = claim.engine_id AND settlement.docker_root_digest = claim.docker_root_digest AND settlement.snapshot_sha256 = claim.snapshot_sha256 AND settlement.claim_generation = claim.claim_generation AND settlement.outcome = 'capacity_available')",
            (
                claim.reservation_id(),
                claim.connection_id(),
                claim.operation_id(),
                claim.engine_id(),
                claim.docker_root_digest(),
                network_name,
                network_id,
                claim.snapshot_sha256(),
                claim.generation(),
                claim.lease_until(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

async fn capacity_available_settlement(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    claim: &PreJitCleanupClaim,
) -> Result<Option<(i64, i64, String, String, Option<String>)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT current.lineage_id, current.generation, settlement.sample_evidence_sha256, network.network_name, network.network_id FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id JOIN capacity_reservations AS reservation ON reservation.id = current.reservation_id JOIN connection_intents AS intent ON intent.id = current.intent_id JOIN connection_request_authorizations AS auth ON auth.connection_id = attempt.connection_id AND auth.scale_set_id = attempt.scale_set_id AND auth.request_id = attempt.witness_request_id JOIN capacity_reservation_pre_jit_cleanup_claims AS claim ON claim.reservation_id = reservation.id JOIN capacity_reservation_pre_jit_cleanup_progress AS progress ON progress.reservation_id = claim.reservation_id AND progress.claim_generation = claim.claim_generation JOIN capacity_reservation_pre_jit_cleanup_settlements AS settlement ON settlement.reservation_id = claim.reservation_id AND settlement.claim_generation = claim.claim_generation JOIN resource_probe_operations AS probe ON probe.operation_id = settlement.probe_operation_id JOIN host_operation_leases AS lease ON lease.singleton = 1 AND lease.operation_id = probe.operation_id JOIN host_storage_profile_current AS current_profile ON current_profile.singleton = 1 AND current_profile.generation_id = settlement.profile_generation_id JOIN host_storage_profiles AS profile ON profile.generation_id = current_profile.generation_id JOIN capacity_reservation_networks AS network ON network.reservation_id = reservation.id WHERE current.reservation_id = ?1 AND current.intent_id = ?2 AND attempt.connection_id = ?3 AND attempt.binding_digest = ?9 AND attempt.engine_id = ?4 AND attempt.docker_root_digest = ?5 AND reservation.state = 'uncertain' AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0 AND intent.state = 'uncertain' AND intent.cleanup_proven = 0 AND intent.jit_requested = 0 AND intent.github_runner_id IS NULL AND auth.acquire_state = 'acquired' AND NOT EXISTS (SELECT 1 FROM connection_runner_jits AS jit WHERE jit.intent_id = attempt.intent_id) AND NOT EXISTS (SELECT 1 FROM execution_publication_operations AS publication WHERE publication.reservation_id = reservation.id) AND NOT EXISTS (SELECT 1 FROM connection_execution_proofs AS proof WHERE proof.reservation_id = reservation.id) AND claim.connection_id = ?3 AND claim.operation_id = ?2 AND claim.engine_id = ?4 AND claim.docker_root_digest = ?5 AND claim.snapshot_sha256 = ?6 AND claim.claim_generation = ?7 AND claim.lease_until = ?8 AND claim.lease_until > CAST(strftime('%s', 'now') AS INTEGER) AND claim.phase = 'claimed' AND progress.connection_id = ?3 AND progress.operation_id = ?2 AND progress.engine_id = ?4 AND progress.docker_root_digest = ?5 AND progress.snapshot_sha256 = ?6 AND progress.phase = 'network_absent' AND settlement.connection_id = ?3 AND settlement.operation_id = ?2 AND settlement.engine_id = ?4 AND settlement.docker_root_digest = ?5 AND settlement.snapshot_sha256 = ?6 AND settlement.outcome = 'capacity_available' AND settlement.profile_digest = profile.profile_digest AND network.connection_id = ?3 AND network.operation_id = ?2 AND network.engine_id = ?4 AND network.docker_root_digest = ?5 AND network.create_state = 'created' AND network.remove_state = 'not_requested' AND probe.probe_purpose = 'capacity' AND probe.phase = 'removed' AND probe.engine_id = ?4 AND probe.docker_root_digest = ?5 AND lease.state = 'released' AND lease.engine_id = ?4 AND lease.docker_root_digest = ?5 AND EXISTS (SELECT 1 FROM capacity_reservation_pre_jit_create_responses AS response WHERE response.reservation_id = reservation.id AND response.role = 'runner' AND response.client_api_major = 1 AND response.client_api_minor = 53 AND response.response_status IN (400, 404, 409) AND response.engine_id = ?4 AND response.docker_root_digest = ?5) AND (SELECT COUNT(*) FROM capacity_reservation_pre_jit_cleanup_settlement_pools AS sampled WHERE sampled.reservation_id = settlement.reservation_id AND sampled.claim_generation = settlement.claim_generation) = (SELECT COUNT(*) FROM capacity_reservation_provisioning_pools AS reserved_pool WHERE reserved_pool.reservation_id = reservation.id AND reserved_pool.scope = 'shared-disk' AND reserved_pool.medium = 'disk')",
            (
                claim.reservation_id(),
                claim.operation_id(),
                claim.connection_id(),
                claim.engine_id(),
                claim.docker_root_digest(),
                claim.snapshot_sha256(),
                claim.generation(),
                claim.lease_until(),
                scope.binding_digest(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let result = (
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
        row.get(2).map_err(|_| HostError::Journal)?,
        row.get(3).map_err(|_| HostError::Journal)?,
        row.get(4).map_err(|_| HostError::Journal)?,
    );
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(result))
}

async fn insert_terminal_outcome(
    connection: &turso::Connection,
    lineage_id: i64,
    generation: i64,
    reservation_id: i64,
    digest: &str,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "INSERT INTO physical_attempt_outcomes (lineage_id, generation, reservation_id, outcome, proof_digest, settlement_digest, created_at) VALUES (?1, ?2, ?3, 'pre_jit_capacity_available', ?4, ?4, unixepoch())",
            (lineage_id, generation, reservation_id, digest),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn release_reservation(
    connection: &turso::Connection,
    claim: &PreJitCleanupClaim,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE capacity_reservations SET state = 'released', remote_cleanup_proven = 1, local_cleanup_proven = 1 WHERE id = ?1 AND connection_id = ?2 AND intent_id = ?3 AND state = 'uncertain' AND remote_cleanup_proven = 0 AND local_cleanup_proven = 0",
            (claim.reservation_id(), claim.connection_id(), claim.operation_id()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn remove_current_attempt(
    connection: &turso::Connection,
    lineage_id: i64,
    generation: i64,
    claim: &PreJitCleanupClaim,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "DELETE FROM physical_attempt_current WHERE lineage_id = ?1 AND generation = ?2 AND reservation_id = ?3 AND intent_id = ?4",
            (lineage_id, generation, claim.reservation_id(), claim.operation_id()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
