use sha2::{Digest, Sha256};

use crate::error::HostError;

pub(super) async fn matches_terminal_settlement(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    intent_id: i64,
    claim_generation: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT attempt.lineage_id, attempt.generation, attempt.reservation_id, attempt.binding_digest, state.instance_binding_digest, cleanup.runner_request_id, cleanup.runner_id, cleanup.runner_name, auth.completed_event_digest, outcome.proof_digest, outcome.settlement_digest FROM physical_attempts AS attempt JOIN physical_attempt_lineages AS lineage ON lineage.lineage_id = attempt.lineage_id JOIN capacity_reservations AS reservation ON reservation.id = attempt.reservation_id AND reservation.connection_id = attempt.connection_id AND reservation.intent_id = attempt.intent_id JOIN connection_intents AS intent ON intent.id = attempt.intent_id AND intent.connection_id = attempt.connection_id JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = reservation.id AND provisioning.operation_id = intent.id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id JOIN capacity_reservation_provisioning_cleanups AS cleanup_proof ON cleanup_proof.reservation_id = reservation.id JOIN capacity_reservation_cleanup_observer_witnesses AS witness ON witness.reservation_id = reservation.id AND witness.connection_id = reservation.connection_id AND witness.operation_id = intent.id JOIN connection_completion_cleanup AS cleanup ON cleanup.intent_id = intent.id AND cleanup.connection_id = intent.connection_id AND cleanup.scale_set_id = intent.scale_set_id JOIN connection_request_authorizations AS auth ON auth.connection_id = cleanup.connection_id AND auth.scale_set_id = cleanup.scale_set_id AND auth.request_id = cleanup.runner_request_id JOIN connection_request_authorizations AS acquire_auth ON acquire_auth.connection_id = attempt.connection_id AND acquire_auth.scale_set_id = attempt.scale_set_id AND acquire_auth.request_id = attempt.witness_request_id JOIN connection_runner_jits AS jit ON jit.intent_id = intent.id AND jit.connection_id = intent.connection_id JOIN capacity_reservation_metadata_settlement_claims AS metadata_claim ON metadata_claim.reservation_id = reservation.id AND metadata_claim.operation_id = intent.id AND metadata_claim.connection_id = attempt.connection_id AND metadata_claim.scale_set_id = attempt.scale_set_id JOIN capacity_reservation_metadata_settlements AS metadata_settlement ON metadata_settlement.reservation_id = metadata_claim.reservation_id AND metadata_settlement.generation = metadata_claim.generation AND metadata_settlement.probe_operation_id = metadata_claim.probe_operation_id JOIN physical_attempt_outcomes AS outcome ON outcome.lineage_id = attempt.lineage_id AND outcome.generation = attempt.generation AND outcome.reservation_id = reservation.id WHERE attempt.connection_id = ?1 AND attempt.scale_set_id = ?2 AND attempt.intent_id = ?3 AND attempt.binding_digest = ?4 AND cleanup.claim_generation = ?5 AND cleanup.runner_absent = 1 AND reservation.state = 'released' AND reservation.remote_cleanup_proven = 1 AND reservation.local_cleanup_proven = 1 AND reservation.binding_digest = attempt.binding_digest AND reservation.engine_id = attempt.engine_id AND reservation.docker_root_digest = attempt.docker_root_digest AND lineage.connection_id = attempt.connection_id AND lineage.scale_set_id = attempt.scale_set_id AND lineage.binding_digest = attempt.binding_digest AND intent.kind = 'launch' AND intent.scale_set_id = attempt.scale_set_id AND intent.cleanup_proven = 1 AND intent.runner_completed = 1 AND intent.runner_request_id = cleanup.runner_request_id AND intent.runner_name = cleanup.runner_name AND intent.github_runner_id = CAST(cleanup.runner_id AS TEXT) AND state.bootstrap_start_state = 'started' AND state.resources_bound = 1 AND state.storage_cleanup_proven = 1 AND state.jit_state = 'cleaned' AND state.runner_id = cleanup.runner_id AND state.runner_name = cleanup.runner_name AND state.runner_scale_set_id = attempt.scale_set_id AND state.instance_binding_digest = cleanup_proof.instance_binding_digest AND state.instance_binding_digest = witness.instance_binding_digest AND state.guest_boot_id = witness.guest_boot_id AND state.runner_container_id = witness.runner_container_id AND state.dind_container_id = witness.dind_container_id AND cleanup_proof.runner_container_id = state.runner_container_id AND cleanup_proof.dind_container_id = state.dind_container_id AND cleanup_proof.role_mask = 2047 AND cleanup_proof.containers_absent = 1 AND cleanup_proof.volumes_absent = 1 AND cleanup_proof.mounts_detached = 1 AND cleanup_proof.cgroups_absent = 1 AND cleanup_proof.instance_binding_digest = state.instance_binding_digest AND cleanup.runner_id > 0 AND cleanup.runner_name != '' AND auth.binding_digest = attempt.binding_digest AND auth.acquire_state = 'acquired' AND auth.completed_event_digest IS NOT NULL AND auth.completed_intent_id = intent.id AND auth.completed_runner_id = cleanup.runner_id AND auth.completed_runner_name = cleanup.runner_name AND acquire_auth.binding_digest = attempt.binding_digest AND acquire_auth.source_digest = attempt.source_digest AND acquire_auth.acquire_state = 'acquired' AND acquire_auth.acquire_response_digest IS NOT NULL AND jit.scale_set_id = attempt.scale_set_id AND jit.state = 'cleaned' AND jit.witness_request_id = attempt.witness_request_id AND jit.completed_request_id = cleanup.runner_request_id AND jit.runner_id = cleanup.runner_id AND jit.runner_name = cleanup.runner_name AND metadata_claim.generation = (SELECT MAX(latest.generation) FROM capacity_reservation_metadata_settlement_claims AS latest WHERE latest.reservation_id = reservation.id) AND metadata_claim.engine_id = attempt.engine_id AND metadata_claim.docker_root_digest = attempt.docker_root_digest AND metadata_claim.instance_binding_digest = state.instance_binding_digest AND metadata_settlement.sample_generation_id = metadata_claim.probe_operation_id AND metadata_settlement.profile_generation_id = metadata_claim.profile_generation_id AND metadata_settlement.profile_digest = metadata_claim.profile_digest AND metadata_settlement.engine_id = metadata_claim.engine_id AND metadata_settlement.docker_root_digest = metadata_claim.docker_root_digest AND metadata_settlement.outcome = 'settled' AND outcome.outcome = 'normal_completion' AND NOT EXISTS (SELECT 1 FROM physical_attempt_current AS current WHERE current.lineage_id = attempt.lineage_id AND current.generation = attempt.generation AND current.reservation_id = attempt.reservation_id) AND NOT EXISTS (SELECT 1 FROM physical_attempt_current AS current WHERE current.reservation_id = reservation.id)",
            (
                connection_id,
                scale_set_id,
                intent_id,
                binding_digest,
                claim_generation,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let lineage_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let attempt_generation: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let reservation_id: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    let attempt_binding: String = row.get(3).map_err(|_| HostError::Journal)?;
    let instance_binding: String = row.get(4).map_err(|_| HostError::Journal)?;
    let request_id: i64 = row.get(5).map_err(|_| HostError::Journal)?;
    let runner_id: i64 = row.get(6).map_err(|_| HostError::Journal)?;
    let runner_name: String = row.get(7).map_err(|_| HostError::Journal)?;
    let event_digest: String = row.get(8).map_err(|_| HostError::Journal)?;
    let proof_digest: String = row.get(9).map_err(|_| HostError::Journal)?;
    let settlement_digest: String = row.get(10).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || lineage_id <= 0
        || reservation_id <= 0
        || attempt_binding != binding_digest
        || instance_binding.len() != 64
        || request_id <= 0
        || runner_id <= 0
        || runner_name.is_empty()
        || event_digest.len() != 64
    {
        return Ok(false);
    }
    let expected = completion_settlement_digest(&CompletionSettlementDigest {
        connection_id,
        reservation_id,
        intent_id,
        lineage_id,
        attempt_generation,
        binding_digest: &attempt_binding,
        instance_binding_digest: &instance_binding,
        request_id,
        runner_id,
        runner_name: &runner_name,
        event_digest: &event_digest,
    });
    Ok(proof_digest == expected && settlement_digest == expected)
}

pub(super) async fn retire_terminal_claim(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    intent_id: i64,
    generation: i64,
) -> Result<bool, HostError> {
    let changed = connection
        .execute(
            "UPDATE connection_completion_cleanup SET retry_after = 0, lease_until = 0 WHERE intent_id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND claim_generation = ?4 AND runner_absent = 1 AND EXISTS (SELECT 1 FROM connection_intents WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND kind = 'launch' AND cleanup_proven = 1)",
            (intent_id, connection_id, scale_set_id, generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(changed == 1)
}

struct CompletionSettlementDigest<'a> {
    connection_id: &'a str,
    reservation_id: i64,
    intent_id: i64,
    lineage_id: i64,
    attempt_generation: i64,
    binding_digest: &'a str,
    instance_binding_digest: &'a str,
    request_id: i64,
    runner_id: i64,
    runner_name: &'a str,
    event_digest: &'a str,
}

fn completion_settlement_digest(identity: &CompletionSettlementDigest<'_>) -> String {
    let canonical = format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        identity.connection_id,
        identity.reservation_id,
        identity.intent_id,
        identity.lineage_id,
        identity.attempt_generation,
        identity.binding_digest,
        identity.instance_binding_digest,
        identity.request_id,
        identity.runner_id,
        identity.runner_name,
        identity.event_digest,
    );
    crate::digest_hex::lowercase_hex(&Sha256::digest(canonical.as_bytes()))
}
