pub(in crate::journal::schema::jit_conflict_settlement) const METADATA_JIT_RECOVERY_PROOF_GUARD:
    &str = r"CREATE TRIGGER jit_conflict_metadata_claim_proof_guard_recovery_core BEFORE INSERT ON capacity_reservation_metadata_settlement_claims
WHEN EXISTS(
    SELECT 1 FROM capacity_reservation_provisioning_state candidate
    WHERE candidate.reservation_id=NEW.reservation_id AND candidate.jit_state='uncertain'
) AND NOT EXISTS(
    SELECT 1
    FROM capacity_reservations r
    JOIN connection_intents i ON i.id=r.intent_id AND i.connection_id=r.connection_id
    JOIN capacity_reservation_provisioning_state s ON s.reservation_id=r.id
    JOIN capacity_reservation_provisioning_cleanups c ON c.reservation_id=r.id
    JOIN physical_attempt_jit_conflict_recovery_claims conflict ON conflict.reservation_id=r.id
    JOIN physical_attempt_current current ON current.lineage_id=conflict.lineage_id AND current.generation=conflict.generation AND current.reservation_id=conflict.reservation_id AND current.intent_id=conflict.intent_id
    JOIN physical_attempts attempt ON attempt.lineage_id=current.lineage_id AND attempt.generation=current.generation AND attempt.reservation_id=current.reservation_id AND attempt.intent_id=current.intent_id
    JOIN physical_attempt_jit_conflict_receipts receipt ON receipt.lineage_id=conflict.lineage_id AND receipt.generation=conflict.generation AND receipt.reservation_id=conflict.reservation_id AND receipt.intent_id=conflict.intent_id
    JOIN connection_runner_jits jit ON jit.intent_id=conflict.intent_id AND jit.connection_id=conflict.connection_id
    JOIN connection_request_authorizations auth ON auth.connection_id=conflict.connection_id AND auth.scale_set_id=conflict.scale_set_id AND auth.request_id=conflict.witness_request_id
    JOIN connection_active_job_authority active ON active.connection_id=conflict.connection_id
    JOIN connection_job_authority_policies policy ON policy.connection_id=active.connection_id AND policy.generation=active.generation AND policy.policy_digest=active.policy_digest
    JOIN capacity_reservation_networks network ON network.reservation_id=conflict.reservation_id AND network.connection_id=conflict.connection_id AND network.operation_id=conflict.intent_id
    JOIN physical_attempt_jit_conflict_recovery_progress local ON local.reservation_id=conflict.reservation_id AND local.claim_generation=conflict.claim_generation AND local.phase_index=1 AND local.phase='local_absent'
    JOIN physical_attempt_jit_conflict_recovery_progress remote ON remote.reservation_id=conflict.reservation_id AND remote.claim_generation=conflict.claim_generation AND remote.phase_index=4 AND remote.phase='remote_absent'
    WHERE ((r.id=NEW.reservation_id ))
      AND (( i.id=NEW.operation_id ))
      AND (( i.connection_id=NEW.connection_id ))
      AND (( i.scale_set_id=NEW.scale_set_id
      ))
      AND (( conflict.reservation_id=r.id ))
      AND (( conflict.intent_id=i.id ))
      AND (( conflict.connection_id=NEW.connection_id ))
      AND (( conflict.scale_set_id=NEW.scale_set_id
      ))
      AND (( conflict.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_jit_conflict_recovery_claims latest WHERE latest.reservation_id=conflict.reservation_id)
      ))
      AND (( conflict.lease_until>unixepoch() ))
      AND (( conflict.engine_id=NEW.engine_id ))
      AND (( conflict.docker_root_digest=NEW.docker_root_digest ))
      AND (( s.instance_binding_digest=NEW.instance_binding_digest
      ))
      AND (( conflict.requested_name=i.runner_name ))
      AND (( conflict.requested_name=jit.requested_name ))
      AND (( conflict.requested_name='v'||attempt.physical_nonce
      ))
      AND (( attempt.connection_id=conflict.connection_id ))
      AND (( attempt.scale_set_id=conflict.scale_set_id ))
      AND (( attempt.witness_request_id=conflict.witness_request_id
      ))
      AND (( attempt.physical_nonce=conflict.physical_nonce ))
      AND (( attempt.binding_digest=conflict.binding_digest ))
      AND (( attempt.source_digest=conflict.source_digest ))
      AND (( (attempt.acquire_response_digest IS NULL OR attempt.acquire_response_digest=conflict.acquire_response_digest)
      ))
      AND (( attempt.engine_id=conflict.engine_id ))
      AND (( attempt.docker_root_digest=conflict.docker_root_digest
      ))
      AND (( r.state='uncertain' ))
      AND (( i.kind='launch' ))
      AND (( i.state='uncertain' ))
      AND (( i.launch_phase='jit_uncertain' ))
      AND (( i.cleanup_proven=0 ))
      AND (( i.jit_requested=1 ))
      AND (( i.github_runner_id IS NULL
      ))
      AND (( jit.scale_set_id=conflict.scale_set_id ))
      AND (( jit.witness_request_id=conflict.witness_request_id ))
      AND (( jit.state='uncertain' ))
      AND (( jit.runner_id IS NULL ))
      AND (( jit.runner_name IS NULL ))
      AND (( jit.started_request_id IS NULL ))
      AND (( jit.completed_request_id IS NULL
      ))
      AND (( s.resources_bound=1 ))
      AND (( s.storage_cleanup_proven=1 ))
      AND (( s.jit_state='uncertain' ))
)
BEGIN SELECT RAISE(ABORT,'invalid metadata settlement claim'); END";

pub(in crate::journal::schema::jit_conflict_settlement) const METADATA_JIT_RECOVERY_DETAIL_PROOF_GUARD: &str = r"CREATE TRIGGER jit_conflict_metadata_claim_proof_guard_recovery_detail BEFORE INSERT ON capacity_reservation_metadata_settlement_claims
WHEN EXISTS(
    SELECT 1 FROM capacity_reservation_provisioning_state candidate
    WHERE candidate.reservation_id=NEW.reservation_id AND candidate.jit_state='uncertain'
) AND NOT EXISTS(
    SELECT 1
    FROM capacity_reservations r
    JOIN connection_intents i ON i.id=r.intent_id AND i.connection_id=r.connection_id
    JOIN capacity_reservation_provisioning_state s ON s.reservation_id=r.id
    JOIN capacity_reservation_provisioning_cleanups c ON c.reservation_id=r.id
    JOIN physical_attempt_jit_conflict_recovery_claims conflict ON conflict.reservation_id=r.id
    JOIN physical_attempt_current current ON current.lineage_id=conflict.lineage_id AND current.generation=conflict.generation AND current.reservation_id=conflict.reservation_id AND current.intent_id=conflict.intent_id
    JOIN physical_attempts attempt ON attempt.lineage_id=current.lineage_id AND attempt.generation=current.generation AND attempt.reservation_id=current.reservation_id AND attempt.intent_id=current.intent_id
    JOIN physical_attempt_jit_conflict_receipts receipt ON receipt.lineage_id=conflict.lineage_id AND receipt.generation=conflict.generation AND receipt.reservation_id=conflict.reservation_id AND receipt.intent_id=conflict.intent_id
    JOIN connection_runner_jits jit ON jit.intent_id=conflict.intent_id AND jit.connection_id=conflict.connection_id
    JOIN connection_request_authorizations auth ON auth.connection_id=conflict.connection_id AND auth.scale_set_id=conflict.scale_set_id AND auth.request_id=conflict.witness_request_id
    JOIN connection_active_job_authority active ON active.connection_id=conflict.connection_id
    JOIN connection_job_authority_policies policy ON policy.connection_id=active.connection_id AND policy.generation=active.generation AND policy.policy_digest=active.policy_digest
    JOIN capacity_reservation_networks network ON network.reservation_id=conflict.reservation_id AND network.connection_id=conflict.connection_id AND network.operation_id=conflict.intent_id
    JOIN physical_attempt_jit_conflict_recovery_progress local ON local.reservation_id=conflict.reservation_id AND local.claim_generation=conflict.claim_generation AND local.phase_index=1 AND local.phase='local_absent'
    JOIN physical_attempt_jit_conflict_recovery_progress remote ON remote.reservation_id=conflict.reservation_id AND remote.claim_generation=conflict.claim_generation AND remote.phase_index=4 AND remote.phase='remote_absent'
    WHERE ((r.id=NEW.reservation_id ))
      AND (( i.id=NEW.operation_id ))
      AND (( i.connection_id=NEW.connection_id ))
      AND (( i.scale_set_id=NEW.scale_set_id
      ))
      AND (( conflict.reservation_id=r.id ))
      AND (( conflict.intent_id=i.id ))
      AND (( conflict.connection_id=NEW.connection_id ))
      AND (( conflict.scale_set_id=NEW.scale_set_id
      ))
      AND (( conflict.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_jit_conflict_recovery_claims latest WHERE latest.reservation_id=conflict.reservation_id)
      ))
      AND (( s.runner_id IS NULL ))
      AND (( s.runner_name IS NULL ))
      AND (( s.runner_scale_set_id IS NULL
      ))
      AND (( c.instance_binding_digest=s.instance_binding_digest
      ))
      AND (( network.create_state='created' ))
      AND (( network.remove_state='removed' ))
      AND (( network.engine_id=conflict.engine_id ))
      AND (( network.docker_root_digest=conflict.docker_root_digest
      ))
      AND (( receipt.binding_digest=conflict.binding_digest ))
      AND (( receipt.source_digest=conflict.source_digest ))
      AND (( receipt.acquire_response_digest=conflict.acquire_response_digest
      ))
      AND (( receipt.authority_generation=conflict.authority_generation ))
      AND (( receipt.authority_digest=conflict.authority_digest ))
      AND (( receipt.engine_id=conflict.engine_id ))
      AND (( receipt.docker_root_digest=conflict.docker_root_digest
      ))
      AND (( receipt.requested_name=conflict.requested_name ))
      AND (( receipt.response_status=409
      ))
      AND (( auth.acquire_state='acquired' ))
      AND (( auth.binding_digest=conflict.binding_digest ))
      AND (( auth.source_digest=conflict.source_digest ))
      AND (( auth.acquire_response_digest=conflict.acquire_response_digest
      ))
      AND (( auth.started_event_digest IS NULL ))
      AND (( auth.completed_event_digest IS NULL
      ))
      AND (( active.generation=conflict.authority_generation ))
      AND (( active.policy_digest=conflict.authority_digest ))
      AND (( policy.binding_digest=conflict.binding_digest
      ))
      AND (( local.evidence_sha256 IS NOT NULL ))
      AND (( remote.runner_name=conflict.requested_name ))
      AND (( remote.runner_scale_set_id=conflict.scale_set_id ))
      AND (( remote.lookup_status=200 ))
      AND (( remote.lookup_count=0 ))
      AND (( remote.request_sha256 IS NOT NULL ))
      AND (( remote.response_sha256 IS NOT NULL
      ))
)
BEGIN SELECT RAISE(ABORT,'invalid metadata settlement claim'); END";
