use crate::error::HostError;

use super::{SettlementInput, settlement_digest};

pub(in crate::journal::connection::reservation::jit_conflict) async fn validate_persisted_settlements(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT settlement.proof_digest,settlement.settlement_digest,claim.lineage_id,claim.generation,settlement.reservation_id,claim.intent_id,settlement.claim_generation,settlement.metadata_generation,claim.connection_id,claim.scale_set_id,claim.witness_request_id,claim.physical_nonce,claim.binding_digest,claim.source_digest,claim.acquire_response_digest,claim.authority_generation,claim.authority_digest,claim.engine_id,claim.docker_root_digest,claim.requested_name,claim.snapshot_sha256,local.evidence_sha256,remote.evidence_sha256,settlement.remote_runner_id,remote.runner_name,remote.runner_scale_set_id,metadata_claim.probe_operation_id,metadata.sample_generation_id,metadata.profile_generation_id,metadata.profile_digest,metadata.observed_at_unix_ms FROM physical_attempt_jit_conflict_settlements settlement LEFT JOIN physical_attempt_jit_conflict_recovery_claims claim ON claim.reservation_id=settlement.reservation_id AND claim.claim_generation=settlement.claim_generation LEFT JOIN physical_attempt_jit_conflict_recovery_progress local ON local.reservation_id=settlement.reservation_id AND local.claim_generation=settlement.claim_generation AND local.phase_index=1 AND local.phase='local_absent' LEFT JOIN physical_attempt_jit_conflict_recovery_progress remote ON remote.reservation_id=settlement.reservation_id AND remote.claim_generation=settlement.claim_generation AND remote.phase_index=4 AND remote.phase='remote_absent' LEFT JOIN capacity_reservation_metadata_settlement_claims metadata_claim ON metadata_claim.reservation_id=settlement.reservation_id AND metadata_claim.generation=settlement.metadata_generation LEFT JOIN capacity_reservation_metadata_settlements metadata ON metadata.reservation_id=metadata_claim.reservation_id AND metadata.generation=metadata_claim.generation ORDER BY settlement.lineage_id,settlement.generation",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let proof_digest: String = row.get(0).map_err(|_| HostError::Journal)?;
        let stored_digest: String = row.get(1).map_err(|_| HostError::Journal)?;
        let input = SettlementInput {
            lineage_id: row.get(2).map_err(|_| HostError::Journal)?,
            generation: row.get(3).map_err(|_| HostError::Journal)?,
            reservation_id: row.get(4).map_err(|_| HostError::Journal)?,
            intent_id: row.get(5).map_err(|_| HostError::Journal)?,
            claim_generation: row.get(6).map_err(|_| HostError::Journal)?,
            metadata_generation: row.get(7).map_err(|_| HostError::Journal)?,
            connection_id: row.get(8).map_err(|_| HostError::Journal)?,
            scale_set_id: row.get(9).map_err(|_| HostError::Journal)?,
            witness_request_id: row.get(10).map_err(|_| HostError::Journal)?,
            physical_nonce: row.get(11).map_err(|_| HostError::Journal)?,
            binding_digest: row.get(12).map_err(|_| HostError::Journal)?,
            source_digest: row.get(13).map_err(|_| HostError::Journal)?,
            acquire_response_digest: row.get(14).map_err(|_| HostError::Journal)?,
            authority_generation: row.get(15).map_err(|_| HostError::Journal)?,
            authority_digest: row.get(16).map_err(|_| HostError::Journal)?,
            engine_id: row.get(17).map_err(|_| HostError::Journal)?,
            docker_root_digest: row.get(18).map_err(|_| HostError::Journal)?,
            requested_name: row.get(19).map_err(|_| HostError::Journal)?,
            snapshot_sha256: row.get(20).map_err(|_| HostError::Journal)?,
            local_evidence_sha256: row.get(21).map_err(|_| HostError::Journal)?,
            remote_evidence_sha256: row.get(22).map_err(|_| HostError::Journal)?,
            runner_id: row.get(23).map_err(|_| HostError::Journal)?,
            runner_name: row.get(24).map_err(|_| HostError::Journal)?,
            remote_scale_set_id: row.get(25).map_err(|_| HostError::Journal)?,
            probe_operation_id: row.get(26).map_err(|_| HostError::Journal)?,
            sample_generation_id: row.get(27).map_err(|_| HostError::Journal)?,
            profile_generation_id: row.get(28).map_err(|_| HostError::Journal)?,
            profile_digest: row.get(29).map_err(|_| HostError::Journal)?,
            observed_at_unix_ms: row.get(30).map_err(|_| HostError::Journal)?,
        };
        if proof_digest != stored_digest || settlement_digest(&input)? != stored_digest {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

pub(in crate::journal::connection::reservation::jit_conflict) async fn metadata_claim_ready(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    if reservation_id <= 0 {
        return Err(HostError::Identity);
    }
    let mut rows = connection
        .query(
            "SELECT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims claim JOIN physical_attempt_current current ON current.lineage_id=claim.lineage_id AND current.generation=claim.generation AND current.reservation_id=claim.reservation_id AND current.intent_id=claim.intent_id JOIN physical_attempts attempt ON attempt.lineage_id=claim.lineage_id AND attempt.generation=claim.generation AND attempt.reservation_id=claim.reservation_id AND attempt.intent_id=claim.intent_id JOIN capacity_reservations reservation ON reservation.id=claim.reservation_id AND reservation.intent_id=claim.intent_id AND reservation.connection_id=claim.connection_id JOIN connection_intents intent ON intent.id=claim.intent_id AND intent.connection_id=claim.connection_id JOIN connection_runner_jits jit ON jit.intent_id=claim.intent_id AND jit.connection_id=claim.connection_id JOIN capacity_reservation_provisioning_state state ON state.reservation_id=claim.reservation_id JOIN capacity_reservation_provisioning_cleanups cleanup ON cleanup.reservation_id=claim.reservation_id JOIN capacity_reservation_networks network ON network.reservation_id=claim.reservation_id AND network.connection_id=claim.connection_id AND network.operation_id=claim.intent_id JOIN physical_attempt_jit_conflict_receipts receipt ON receipt.lineage_id=claim.lineage_id AND receipt.generation=claim.generation AND receipt.reservation_id=claim.reservation_id AND receipt.intent_id=claim.intent_id JOIN connection_request_authorizations auth ON auth.connection_id=claim.connection_id AND auth.scale_set_id=claim.scale_set_id AND auth.request_id=claim.witness_request_id JOIN connection_active_job_authority active ON active.connection_id=claim.connection_id JOIN connection_job_authority_policies policy ON policy.connection_id=active.connection_id AND policy.generation=active.generation AND policy.policy_digest=active.policy_digest JOIN physical_attempt_jit_conflict_recovery_progress local ON local.reservation_id=claim.reservation_id AND local.claim_generation=claim.claim_generation AND local.phase_index=1 AND local.phase='local_absent' JOIN physical_attempt_jit_conflict_recovery_progress remote ON remote.reservation_id=claim.reservation_id AND remote.claim_generation=claim.claim_generation AND remote.phase_index=4 AND remote.phase='remote_absent' WHERE claim.reservation_id=?1 AND claim.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_jit_conflict_recovery_claims latest WHERE latest.reservation_id=claim.reservation_id) AND claim.lease_until>unixepoch() AND attempt.connection_id=claim.connection_id AND attempt.scale_set_id=claim.scale_set_id AND attempt.witness_request_id=claim.witness_request_id AND attempt.physical_nonce=claim.physical_nonce AND attempt.binding_digest=claim.binding_digest AND attempt.source_digest=claim.source_digest AND (attempt.acquire_response_digest IS NULL OR attempt.acquire_response_digest=claim.acquire_response_digest) AND attempt.engine_id=claim.engine_id AND attempt.docker_root_digest=claim.docker_root_digest AND reservation.state='uncertain' AND reservation.remote_cleanup_proven=1 AND reservation.local_cleanup_proven=1 AND intent.kind='launch' AND intent.state='uncertain' AND intent.launch_phase='jit_uncertain' AND intent.cleanup_proven=0 AND intent.jit_requested=1 AND intent.github_runner_id IS NULL AND intent.runner_name=claim.requested_name AND jit.state='uncertain' AND jit.runner_id IS NULL AND jit.runner_name IS NULL AND jit.scale_set_id=claim.scale_set_id AND jit.witness_request_id=claim.witness_request_id AND jit.requested_name=claim.requested_name AND jit.started_request_id IS NULL AND jit.completed_request_id IS NULL AND state.resources_bound=1 AND state.storage_cleanup_proven=1 AND state.jit_state='uncertain' AND state.runner_id IS NULL AND state.runner_name IS NULL AND state.runner_scale_set_id IS NULL AND cleanup.instance_binding_digest=state.instance_binding_digest AND network.create_state='created' AND network.remove_state='removed' AND network.engine_id=claim.engine_id AND network.docker_root_digest=claim.docker_root_digest AND receipt.requested_name=claim.requested_name AND receipt.response_status=409 AND receipt.authority_generation=claim.authority_generation AND receipt.authority_digest=claim.authority_digest AND auth.acquire_state='acquired' AND auth.acquire_response_digest=claim.acquire_response_digest AND auth.binding_digest=claim.binding_digest AND auth.source_digest=claim.source_digest AND auth.started_event_digest IS NULL AND auth.completed_event_digest IS NULL AND active.generation=claim.authority_generation AND active.policy_digest=claim.authority_digest AND policy.binding_digest=claim.binding_digest AND remote.runner_name=claim.requested_name AND remote.runner_scale_set_id=claim.scale_set_id AND remote.lookup_status=200 AND remote.lookup_count=0 AND remote.response_sha256 IS NOT NULL AND NOT EXISTS(SELECT 1 FROM connection_completion_inbox inbox WHERE inbox.connection_id=claim.connection_id AND inbox.scale_set_id=claim.scale_set_id) AND NOT EXISTS(SELECT 1 FROM connection_request_authorizations event_auth WHERE event_auth.connection_id=claim.connection_id AND event_auth.scale_set_id=claim.scale_set_id AND ((event_auth.started_event_digest IS NOT NULL AND event_auth.started_runner_name=claim.requested_name) OR (event_auth.completed_event_digest IS NOT NULL AND event_auth.completed_runner_name=claim.requested_name))) AND NOT EXISTS(SELECT 1 FROM connection_execution_decisions decision WHERE decision.intent_id=claim.intent_id) AND NOT EXISTS(SELECT 1 FROM execution_publication_operations publication WHERE publication.reservation_id=claim.reservation_id) AND NOT EXISTS(SELECT 1 FROM connection_execution_proofs proof WHERE proof.reservation_id=claim.reservation_id))",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HostError::Journal),
    }
}
