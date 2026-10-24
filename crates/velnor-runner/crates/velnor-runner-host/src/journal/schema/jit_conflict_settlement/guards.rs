//! SQL fences for exact JIT conflict recovery proof.

use crate::error::HostError;

mod recovery_proof;
pub(super) use recovery_proof::{
    METADATA_JIT_RECOVERY_DETAIL_PROOF_GUARD, METADATA_JIT_RECOVERY_PROOF_GUARD,
};

const LEGACY_METADATA_JIT_GATE: &str = "s.jit_state IN ('not_requested', 'cleaned')";
const V40_METADATA_JIT_GATE: &str = "s.jit_state IN ('not_requested', 'cleaned', 'uncertain')";

pub(super) const METADATA_JIT_REMOTE_HISTORY_PROOF_GUARD: &str = r"CREATE TRIGGER jit_conflict_metadata_claim_proof_guard_remote_history BEFORE INSERT ON capacity_reservation_metadata_settlement_claims
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
    WHERE (r.id=NEW.reservation_id) AND
      (i.id=NEW.operation_id) AND
      (i.connection_id=NEW.connection_id) AND
      (i.scale_set_id=NEW.scale_set_id) AND
      (conflict.reservation_id=r.id) AND
      (conflict.intent_id=i.id) AND
      (conflict.connection_id=NEW.connection_id) AND
      (conflict.scale_set_id=NEW.scale_set_id) AND
      (conflict.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_jit_conflict_recovery_claims latest WHERE latest.reservation_id=conflict.reservation_id)) AND
      ((remote.runner_id IS NULL AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress initial WHERE initial.reservation_id=conflict.reservation_id AND initial.claim_generation=conflict.claim_generation AND initial.phase_index=2 AND initial.phase='remote_initial_absent' AND initial.runner_id IS NULL AND initial.runner_name=conflict.requested_name AND initial.runner_scale_set_id=conflict.scale_set_id AND initial.lookup_status=200 AND initial.lookup_count=0 AND initial.request_sha256 IS NOT NULL AND initial.response_sha256 IS NOT NULL))
           OR (remote.runner_id IS NOT NULL AND (EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress target JOIN physical_attempt_jit_conflict_recovery_progress deletion ON deletion.reservation_id=target.reservation_id AND deletion.claim_generation=target.claim_generation AND deletion.phase_index=3 AND deletion.phase='delete_started' WHERE target.reservation_id=conflict.reservation_id AND target.claim_generation=conflict.claim_generation AND target.phase_index=2 AND target.phase='remote_target' AND target.runner_id=remote.runner_id AND target.runner_name=conflict.requested_name AND target.runner_scale_set_id=conflict.scale_set_id AND deletion.runner_id=remote.runner_id AND deletion.runner_name=conflict.requested_name AND deletion.runner_scale_set_id=conflict.scale_set_id)
               OR EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress target JOIN physical_attempt_jit_conflict_recovery_progress deletion ON deletion.reservation_id=target.reservation_id AND deletion.claim_generation=target.claim_generation AND deletion.phase_index=3 AND deletion.phase='delete_started' WHERE target.reservation_id=conflict.reservation_id AND target.claim_generation<conflict.claim_generation AND target.phase_index=2 AND target.phase='remote_target' AND target.runner_id=remote.runner_id AND target.runner_name=conflict.requested_name AND target.runner_scale_set_id=conflict.scale_set_id AND deletion.runner_id=remote.runner_id AND deletion.runner_name=conflict.requested_name AND deletion.runner_scale_set_id=conflict.scale_set_id AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress prior WHERE prior.reservation_id=conflict.reservation_id AND prior.claim_generation<conflict.claim_generation AND prior.phase_index=2 AND prior.phase='remote_initial_absent')))))
)
BEGIN SELECT RAISE(ABORT,'invalid metadata settlement claim'); END";
pub(super) const METADATA_JIT_REMOTE_NO_EVENTS_PROOF_GUARD: &str = r"CREATE TRIGGER jit_conflict_metadata_claim_proof_guard_remote_no_events BEFORE INSERT ON capacity_reservation_metadata_settlement_claims
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
    WHERE (r.id=NEW.reservation_id) AND
      (i.id=NEW.operation_id) AND
      (i.connection_id=NEW.connection_id) AND
      (i.scale_set_id=NEW.scale_set_id) AND
      (conflict.reservation_id=r.id) AND
      (conflict.intent_id=i.id) AND
      (conflict.connection_id=NEW.connection_id) AND
      (conflict.scale_set_id=NEW.scale_set_id) AND
      (conflict.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_jit_conflict_recovery_claims latest WHERE latest.reservation_id=conflict.reservation_id)) AND
      (NOT EXISTS(SELECT 1 FROM connection_completion_inbox inbox WHERE inbox.connection_id=conflict.connection_id AND inbox.scale_set_id=conflict.scale_set_id)) AND
      (NOT EXISTS(SELECT 1 FROM connection_request_authorizations event_auth WHERE event_auth.connection_id=conflict.connection_id AND event_auth.scale_set_id=conflict.scale_set_id AND ((event_auth.started_event_digest IS NOT NULL AND event_auth.started_runner_name=conflict.requested_name) OR (event_auth.completed_event_digest IS NOT NULL AND event_auth.completed_runner_name=conflict.requested_name)))) AND
      (NOT EXISTS(SELECT 1 FROM connection_execution_decisions decision WHERE decision.intent_id=conflict.intent_id)) AND
      (NOT EXISTS(SELECT 1 FROM execution_publication_operations publication WHERE publication.reservation_id=conflict.reservation_id)) AND
      (NOT EXISTS(SELECT 1 FROM connection_execution_proofs proof WHERE proof.reservation_id=conflict.reservation_id))
)
BEGIN SELECT RAISE(ABORT,'invalid metadata settlement claim'); END";

pub(super) fn metadata_claim_guard_sql_v40() -> Result<String, HostError> {
    let legacy = super::super::provisioning::metadata_settlement_claim_guard_sql()
        .ok_or(HostError::Journal)?;
    replace_once(legacy, LEGACY_METADATA_JIT_GATE, V40_METADATA_JIT_GATE)
}

fn replace_once(source: &str, old: &str, new: &str) -> Result<String, HostError> {
    if source.matches(old).count() != 1 {
        return Err(HostError::Journal);
    }
    Ok(source.replacen(old, new, 1))
}

pub(super) const CLAIM_INSERT_GUARD: &str = r"CREATE TRIGGER physical_attempt_jit_conflict_claim_insert_guard BEFORE INSERT ON physical_attempt_jit_conflict_recovery_claims
WHEN NOT EXISTS (
    SELECT 1 FROM physical_attempt_current cur
    JOIN physical_attempts attempt ON attempt.lineage_id=cur.lineage_id AND attempt.generation=cur.generation AND attempt.reservation_id=cur.reservation_id AND attempt.intent_id=cur.intent_id
    JOIN capacity_reservations reservation ON reservation.id=attempt.reservation_id AND reservation.intent_id=attempt.intent_id AND reservation.connection_id=attempt.connection_id
    JOIN connection_intents intent ON intent.id=attempt.intent_id AND intent.connection_id=attempt.connection_id
    JOIN capacity_reservation_provisioning_state state ON state.reservation_id=reservation.id
    JOIN connection_runner_jits jit ON jit.intent_id=attempt.intent_id AND jit.connection_id=attempt.connection_id
    JOIN connection_request_authorizations auth ON auth.connection_id=attempt.connection_id AND auth.scale_set_id=attempt.scale_set_id AND auth.request_id=attempt.witness_request_id
    JOIN physical_attempt_jit_conflict_receipts receipt ON receipt.lineage_id=attempt.lineage_id AND receipt.generation=attempt.generation AND receipt.reservation_id=attempt.reservation_id AND receipt.intent_id=attempt.intent_id
    JOIN connection_active_job_authority active ON active.connection_id=attempt.connection_id
    JOIN connection_job_authority_policies policy ON policy.connection_id=active.connection_id AND policy.generation=active.generation AND policy.policy_digest=active.policy_digest
    WHERE cur.lineage_id=NEW.lineage_id AND cur.generation=NEW.generation AND cur.reservation_id=NEW.reservation_id AND cur.intent_id=NEW.intent_id
      AND attempt.lineage_id=NEW.lineage_id AND attempt.generation=NEW.generation AND attempt.reservation_id=NEW.reservation_id AND attempt.intent_id=NEW.intent_id
      AND attempt.connection_id=NEW.connection_id AND attempt.scale_set_id=NEW.scale_set_id AND attempt.witness_request_id=NEW.witness_request_id AND attempt.physical_nonce=NEW.physical_nonce
      AND attempt.binding_digest=NEW.binding_digest AND attempt.source_digest=NEW.source_digest AND (attempt.acquire_response_digest IS NULL OR attempt.acquire_response_digest=NEW.acquire_response_digest)
      AND attempt.engine_id=NEW.engine_id AND attempt.docker_root_digest=NEW.docker_root_digest
      AND reservation.state='uncertain' AND reservation.binding_digest=NEW.binding_digest AND reservation.engine_id=NEW.engine_id AND reservation.docker_root_digest=NEW.docker_root_digest
      AND intent.kind='launch' AND intent.state='uncertain' AND intent.launch_phase='jit_uncertain' AND intent.scale_set_id=NEW.scale_set_id AND intent.runner_name=NEW.requested_name AND intent.github_runner_id IS NULL AND intent.cleanup_proven=0 AND intent.jit_requested=1
      AND NEW.requested_name='v'||attempt.physical_nonce
      AND state.resources_bound=1 AND state.jit_state='uncertain' AND state.runner_id IS NULL AND state.runner_name IS NULL AND state.runner_scale_set_id IS NULL AND state.publication_state='not_requested' AND state.delivery_state='not_requested'
      AND ((reservation.local_cleanup_proven=0 AND state.storage_cleanup_proven=0 AND reservation.remote_cleanup_proven=0 AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress prior WHERE prior.reservation_id=reservation.id AND (prior.phase_index=1 OR prior.phase_index=4))) OR (reservation.local_cleanup_proven=1 AND state.storage_cleanup_proven=1 AND EXISTS(SELECT 1 FROM capacity_reservation_provisioning_cleanups cleanup JOIN capacity_reservation_cleanup_observer_witnesses witness ON witness.reservation_id=cleanup.reservation_id WHERE cleanup.reservation_id=reservation.id AND cleanup.instance_binding_digest=state.instance_binding_digest AND cleanup.role_mask=2047 AND cleanup.containers_absent=1 AND cleanup.volumes_absent=1 AND cleanup.mounts_detached=1 AND cleanup.cgroups_absent=1 AND witness.connection_id=attempt.connection_id AND witness.operation_id=attempt.intent_id AND witness.engine_id=attempt.engine_id AND witness.docker_root_digest=attempt.docker_root_digest AND witness.instance_binding_digest=state.instance_binding_digest) AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress local WHERE local.reservation_id=reservation.id AND local.phase_index=1 AND local.phase='local_absent') AND ((reservation.remote_cleanup_proven=0) OR (reservation.remote_cleanup_proven=1 AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress remote WHERE remote.reservation_id=reservation.id AND remote.phase_index=4 AND remote.phase='remote_absent' AND remote.runner_name=NEW.requested_name AND remote.runner_scale_set_id=NEW.scale_set_id AND remote.lookup_status=200 AND remote.lookup_count=0)))))
      AND jit.scale_set_id=NEW.scale_set_id AND jit.witness_request_id=NEW.witness_request_id AND jit.requested_name=NEW.requested_name AND jit.state='uncertain' AND jit.runner_id IS NULL AND jit.runner_name IS NULL AND jit.started_request_id IS NULL AND jit.completed_request_id IS NULL
      AND auth.acquire_state='acquired' AND auth.acquire_response_digest=NEW.acquire_response_digest AND auth.started_event_digest IS NULL AND auth.completed_event_digest IS NULL
      AND active.generation=NEW.authority_generation AND active.policy_digest=NEW.authority_digest AND policy.binding_digest=NEW.binding_digest
      AND receipt.binding_digest=NEW.binding_digest AND receipt.source_digest=NEW.source_digest AND receipt.acquire_response_digest=NEW.acquire_response_digest AND receipt.authority_generation=NEW.authority_generation AND receipt.authority_digest=NEW.authority_digest AND receipt.engine_id=NEW.engine_id AND receipt.docker_root_digest=NEW.docker_root_digest AND receipt.requested_name=NEW.requested_name AND receipt.response_status=409
      AND NEW.lease_until>unixepoch() AND NEW.created_at=unixepoch()
      AND NEW.claim_generation=COALESCE((SELECT MAX(prior.claim_generation)+1 FROM physical_attempt_jit_conflict_recovery_claims prior WHERE prior.reservation_id=NEW.reservation_id),1)
      AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims prior WHERE prior.reservation_id=NEW.reservation_id AND prior.lease_until>unixepoch())
      AND NOT EXISTS(SELECT 1 FROM connection_completion_inbox inbox WHERE inbox.connection_id=NEW.connection_id AND inbox.scale_set_id=NEW.scale_set_id)
      AND NOT EXISTS(SELECT 1 FROM connection_execution_decisions decision WHERE decision.intent_id=NEW.intent_id)
      AND NOT EXISTS(SELECT 1 FROM execution_publication_operations publication WHERE publication.reservation_id=NEW.reservation_id)
      AND NOT EXISTS(SELECT 1 FROM connection_execution_proofs proof WHERE proof.reservation_id=NEW.reservation_id)
)
BEGIN SELECT RAISE(ABORT,'The JIT conflict recovery claim is invalid.'); END";

pub(super) const CLAIM_NO_UPDATE: &str = "CREATE TRIGGER physical_attempt_jit_conflict_claim_no_update BEFORE UPDATE ON physical_attempt_jit_conflict_recovery_claims BEGIN SELECT RAISE(ABORT,'JIT conflict recovery claims are immutable.'); END";
pub(super) const CLAIM_NO_DELETE: &str = "CREATE TRIGGER physical_attempt_jit_conflict_claim_no_delete BEFORE DELETE ON physical_attempt_jit_conflict_recovery_claims BEGIN SELECT RAISE(ABORT,'JIT conflict recovery claims are immutable.'); END";

pub(super) const PROGRESS_INSERT_GUARD: &str = r"CREATE TRIGGER physical_attempt_jit_conflict_progress_insert_guard BEFORE INSERT ON physical_attempt_jit_conflict_recovery_progress
WHEN NOT EXISTS (
    SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims claim
    WHERE claim.reservation_id=NEW.reservation_id AND claim.claim_generation=NEW.claim_generation AND claim.lease_until>unixepoch()
      AND EXISTS(SELECT 1 FROM physical_attempt_current current JOIN physical_attempts attempt ON attempt.lineage_id=current.lineage_id AND attempt.generation=current.generation AND attempt.reservation_id=current.reservation_id AND attempt.intent_id=current.intent_id JOIN capacity_reservations reservation ON reservation.id=claim.reservation_id AND reservation.intent_id=claim.intent_id AND reservation.connection_id=claim.connection_id JOIN connection_intents intent ON intent.id=claim.intent_id AND intent.connection_id=claim.connection_id JOIN connection_runner_jits jit ON jit.intent_id=claim.intent_id AND jit.connection_id=claim.connection_id JOIN physical_attempt_jit_conflict_receipts receipt ON receipt.lineage_id=claim.lineage_id AND receipt.generation=claim.generation AND receipt.reservation_id=claim.reservation_id AND receipt.intent_id=claim.intent_id JOIN connection_request_authorizations auth ON auth.connection_id=claim.connection_id AND auth.scale_set_id=claim.scale_set_id AND auth.request_id=claim.witness_request_id JOIN connection_active_job_authority active ON active.connection_id=claim.connection_id JOIN connection_job_authority_policies policy ON policy.connection_id=active.connection_id AND policy.generation=active.generation AND policy.policy_digest=active.policy_digest WHERE current.lineage_id=claim.lineage_id AND current.generation=claim.generation AND current.reservation_id=claim.reservation_id AND current.intent_id=claim.intent_id AND attempt.connection_id=claim.connection_id AND attempt.scale_set_id=claim.scale_set_id AND attempt.witness_request_id=claim.witness_request_id AND attempt.physical_nonce=claim.physical_nonce AND attempt.binding_digest=claim.binding_digest AND attempt.source_digest=claim.source_digest AND (attempt.acquire_response_digest IS NULL OR attempt.acquire_response_digest=claim.acquire_response_digest) AND attempt.engine_id=claim.engine_id AND attempt.docker_root_digest=claim.docker_root_digest AND reservation.state='uncertain' AND intent.kind='launch' AND intent.state='uncertain' AND intent.launch_phase='jit_uncertain' AND intent.cleanup_proven=0 AND intent.jit_requested=1 AND intent.github_runner_id IS NULL AND intent.runner_name=claim.requested_name AND jit.state='uncertain' AND jit.runner_id IS NULL AND jit.runner_name IS NULL AND jit.requested_name=claim.requested_name AND jit.scale_set_id=claim.scale_set_id AND jit.witness_request_id=claim.witness_request_id AND jit.started_request_id IS NULL AND jit.completed_request_id IS NULL AND receipt.response_status=409 AND receipt.connection_id=claim.connection_id AND receipt.scale_set_id=claim.scale_set_id AND receipt.witness_request_id=claim.witness_request_id AND receipt.requested_name=claim.requested_name AND receipt.binding_digest=claim.binding_digest AND receipt.source_digest=claim.source_digest AND receipt.acquire_response_digest=auth.acquire_response_digest AND receipt.authority_generation=claim.authority_generation AND receipt.authority_digest=claim.authority_digest AND receipt.engine_id=claim.engine_id AND receipt.docker_root_digest=claim.docker_root_digest AND auth.acquire_state='acquired' AND auth.acquire_response_digest=claim.acquire_response_digest AND auth.binding_digest=claim.binding_digest AND auth.source_digest=claim.source_digest AND auth.started_event_digest IS NULL AND auth.completed_event_digest IS NULL AND active.generation=claim.authority_generation AND active.policy_digest=claim.authority_digest AND policy.binding_digest=claim.binding_digest AND NOT EXISTS(SELECT 1 FROM connection_completion_inbox inbox WHERE inbox.connection_id=claim.connection_id AND inbox.scale_set_id=claim.scale_set_id) AND NOT EXISTS(SELECT 1 FROM connection_request_authorizations event_auth WHERE event_auth.connection_id=claim.connection_id AND event_auth.scale_set_id=claim.scale_set_id AND ((event_auth.started_event_digest IS NOT NULL AND event_auth.started_runner_name=claim.requested_name) OR (event_auth.completed_event_digest IS NOT NULL AND event_auth.completed_runner_name=claim.requested_name))) AND NOT EXISTS(SELECT 1 FROM connection_execution_decisions decision WHERE decision.intent_id=claim.intent_id) AND NOT EXISTS(SELECT 1 FROM execution_publication_operations publication WHERE publication.reservation_id=claim.reservation_id) AND NOT EXISTS(SELECT 1 FROM connection_execution_proofs proof WHERE proof.reservation_id=claim.reservation_id))
      AND NEW.observed_at_unix_ms>=claim.created_at*1000
      AND NEW.observed_at_unix_ms<=CAST((julianday('now')-2440587.5)*86400000 AS INTEGER)
      AND (
          NEW.phase_index=COALESCE((SELECT MAX(prior.phase_index)+1 FROM physical_attempt_jit_conflict_recovery_progress prior WHERE prior.reservation_id=claim.reservation_id AND prior.claim_generation=claim.claim_generation),1)
          OR (NEW.phase_index=4
              AND COALESCE((SELECT MAX(prior.phase_index)+1 FROM physical_attempt_jit_conflict_recovery_progress prior WHERE prior.reservation_id=claim.reservation_id AND prior.claim_generation=claim.claim_generation),1)=3
              AND NEW.runner_id IS NULL
              AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress observed WHERE observed.reservation_id=claim.reservation_id AND observed.claim_generation=claim.claim_generation AND observed.phase_index=2 AND observed.phase='remote_initial_absent'))
          OR (NEW.phase_index=4
              AND COALESCE((SELECT MAX(prior.phase_index)+1 FROM physical_attempt_jit_conflict_recovery_progress prior WHERE prior.reservation_id=claim.reservation_id AND prior.claim_generation=claim.claim_generation),1)=2
              AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress local WHERE local.reservation_id=claim.reservation_id AND local.claim_generation=claim.claim_generation AND local.phase_index=1 AND local.phase='local_absent')
              AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress current WHERE current.reservation_id=claim.reservation_id AND current.claim_generation=claim.claim_generation AND current.phase_index=2)
              AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress prior WHERE prior.reservation_id=claim.reservation_id AND prior.claim_generation<claim.claim_generation AND prior.phase_index=2 AND prior.phase='remote_initial_absent')
              AND NEW.runner_id IS NOT NULL AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress deletion JOIN physical_attempt_jit_conflict_recovery_progress target ON target.reservation_id=deletion.reservation_id AND target.claim_generation=deletion.claim_generation AND target.phase_index=2 AND target.phase='remote_target' WHERE deletion.reservation_id=claim.reservation_id AND deletion.claim_generation<claim.claim_generation AND deletion.phase_index=3 AND deletion.phase='delete_started' AND target.runner_id=NEW.runner_id AND target.runner_name=claim.requested_name AND target.runner_scale_set_id=claim.scale_set_id AND NEW.runner_name=target.runner_name AND NEW.runner_scale_set_id=target.runner_scale_set_id))
      )
      AND (
          (NEW.phase_index=1 AND EXISTS(SELECT 1 FROM capacity_reservation_provisioning_state state JOIN capacity_reservation_provisioning_cleanups cleanup ON cleanup.reservation_id=state.reservation_id WHERE state.reservation_id=claim.reservation_id AND state.storage_cleanup_proven=1 AND cleanup.instance_binding_digest=state.instance_binding_digest AND cleanup.role_mask=2047 AND cleanup.containers_absent=1 AND cleanup.volumes_absent=1 AND cleanup.mounts_detached=1 AND cleanup.cgroups_absent=1))
          OR (NEW.phase_index=2 AND NEW.phase='remote_target' AND NEW.runner_id IS NOT NULL AND NEW.runner_name=claim.requested_name AND NEW.runner_scale_set_id=claim.scale_set_id AND NEW.lookup_status=200 AND NEW.lookup_count=1
              AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress local WHERE local.reservation_id=claim.reservation_id AND local.claim_generation=claim.claim_generation AND local.phase_index=1 AND local.phase='local_absent')
              AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress prior WHERE prior.reservation_id=claim.reservation_id AND prior.claim_generation<claim.claim_generation AND prior.phase_index=2 AND (prior.phase='remote_initial_absent' OR prior.runner_id!=NEW.runner_id OR prior.runner_name!=NEW.runner_name OR prior.runner_scale_set_id!=NEW.runner_scale_set_id))
              AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress deletion WHERE deletion.reservation_id=claim.reservation_id AND deletion.phase_index=3 AND deletion.phase='delete_started'))
          OR (NEW.phase_index=2 AND NEW.phase='remote_initial_absent' AND NEW.runner_id IS NULL AND NEW.runner_name=claim.requested_name AND NEW.runner_scale_set_id=claim.scale_set_id AND NEW.lookup_status=200 AND NEW.lookup_count=0
              AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress local WHERE local.reservation_id=claim.reservation_id AND local.claim_generation=claim.claim_generation AND local.phase_index=1 AND local.phase='local_absent')
              AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress prior WHERE prior.reservation_id=claim.reservation_id AND (prior.phase='remote_target' OR prior.phase='delete_started'))
              AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress deletion WHERE deletion.reservation_id=claim.reservation_id AND deletion.phase_index=3 AND deletion.phase='delete_started'))
          OR (NEW.phase_index=3 AND NEW.phase='delete_started' AND NEW.runner_id IS NOT NULL AND NEW.runner_name=claim.requested_name AND NEW.runner_scale_set_id=claim.scale_set_id
              AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress target WHERE target.reservation_id=claim.reservation_id AND target.claim_generation=claim.claim_generation AND target.phase_index=2 AND target.phase='remote_target' AND target.runner_id=NEW.runner_id AND target.runner_name=NEW.runner_name AND target.runner_scale_set_id=NEW.runner_scale_set_id)
              AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress prior WHERE prior.reservation_id=claim.reservation_id AND prior.phase_index=3 AND prior.phase='delete_started'))
          OR (NEW.phase_index=4 AND NEW.phase='remote_absent' AND NEW.runner_id IS NULL AND NEW.runner_name=claim.requested_name AND NEW.runner_scale_set_id=claim.scale_set_id AND NEW.lookup_count=0 AND NEW.lookup_status=200
              AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress observed WHERE observed.reservation_id=claim.reservation_id AND observed.claim_generation=claim.claim_generation AND observed.phase_index=2 AND observed.phase='remote_initial_absent'))
          OR (NEW.phase_index=4 AND NEW.phase='remote_absent' AND NEW.runner_id IS NOT NULL AND NEW.runner_name=claim.requested_name AND NEW.runner_scale_set_id=claim.scale_set_id AND NEW.lookup_count=0 AND NEW.lookup_status=200
              AND (EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress deletion WHERE deletion.reservation_id=claim.reservation_id AND deletion.claim_generation=claim.claim_generation AND deletion.phase_index=3 AND deletion.phase='delete_started' AND deletion.runner_id=NEW.runner_id AND deletion.runner_name=NEW.runner_name AND deletion.runner_scale_set_id=NEW.runner_scale_set_id)
                   OR EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress deletion JOIN physical_attempt_jit_conflict_recovery_progress target ON target.reservation_id=deletion.reservation_id AND target.claim_generation=deletion.claim_generation AND target.phase_index=2 WHERE deletion.reservation_id=claim.reservation_id AND deletion.claim_generation<claim.claim_generation AND deletion.phase_index=3 AND deletion.phase='delete_started' AND target.phase='remote_target' AND target.runner_id=NEW.runner_id AND target.runner_name=NEW.runner_name AND target.runner_scale_set_id=NEW.runner_scale_set_id)))
      )
)
BEGIN SELECT RAISE(ABORT,'The JIT conflict recovery progress is invalid.'); END";

pub(super) const PROGRESS_NO_UPDATE: &str = "CREATE TRIGGER physical_attempt_jit_conflict_progress_no_update BEFORE UPDATE ON physical_attempt_jit_conflict_recovery_progress BEGIN SELECT RAISE(ABORT,'JIT conflict recovery progress is immutable.'); END";
pub(super) const PROGRESS_NO_DELETE: &str = "CREATE TRIGGER physical_attempt_jit_conflict_progress_no_delete BEFORE DELETE ON physical_attempt_jit_conflict_recovery_progress BEGIN SELECT RAISE(ABORT,'JIT conflict recovery progress is immutable.'); END";

pub(super) const SETTLEMENT_INSERT_GUARD: &str = r"CREATE TRIGGER physical_attempt_jit_conflict_settlement_insert_guard BEFORE INSERT ON physical_attempt_jit_conflict_settlements
WHEN NOT EXISTS (
    SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims claim
    JOIN physical_attempt_current current ON current.lineage_id=claim.lineage_id AND current.generation=claim.generation AND current.reservation_id=claim.reservation_id AND current.intent_id=claim.intent_id
    JOIN physical_attempts attempt ON attempt.lineage_id=claim.lineage_id AND attempt.generation=claim.generation AND attempt.reservation_id=claim.reservation_id AND attempt.intent_id=claim.intent_id
    JOIN capacity_reservations reservation ON reservation.id=claim.reservation_id AND reservation.intent_id=claim.intent_id AND reservation.connection_id=claim.connection_id
    JOIN connection_intents intent ON intent.id=claim.intent_id AND intent.connection_id=claim.connection_id
    JOIN connection_runner_jits jit ON jit.intent_id=claim.intent_id AND jit.connection_id=claim.connection_id
    JOIN capacity_reservation_provisioning_state state ON state.reservation_id=claim.reservation_id
    JOIN capacity_reservation_provisioning_cleanups cleanup ON cleanup.reservation_id=claim.reservation_id
    JOIN capacity_reservation_cleanup_observer_witnesses witness ON witness.reservation_id=cleanup.reservation_id AND witness.connection_id=claim.connection_id AND witness.operation_id=claim.intent_id
    JOIN capacity_reservation_networks network ON network.reservation_id=claim.reservation_id AND network.connection_id=claim.connection_id AND network.operation_id=claim.intent_id
    JOIN capacity_reservation_metadata_settlement_claims metadata ON metadata.reservation_id=NEW.reservation_id AND metadata.generation=NEW.metadata_generation
    JOIN capacity_reservation_metadata_settlements sampled ON sampled.reservation_id=metadata.reservation_id AND sampled.generation=metadata.generation AND sampled.probe_operation_id=metadata.probe_operation_id
    JOIN physical_attempt_jit_conflict_recovery_progress local ON local.reservation_id=claim.reservation_id AND local.claim_generation=claim.claim_generation AND local.phase_index=1 AND local.phase='local_absent'
    JOIN physical_attempt_jit_conflict_recovery_progress remote ON remote.reservation_id=claim.reservation_id AND remote.claim_generation=claim.claim_generation AND remote.phase_index=4 AND remote.phase='remote_absent'
    JOIN connection_request_authorizations auth ON auth.connection_id=claim.connection_id AND auth.scale_set_id=claim.scale_set_id AND auth.request_id=claim.witness_request_id
    JOIN connection_active_job_authority active ON active.connection_id=claim.connection_id
    JOIN connection_job_authority_policies policy ON policy.connection_id=active.connection_id AND policy.generation=active.generation AND policy.policy_digest=active.policy_digest
    WHERE claim.reservation_id=NEW.reservation_id AND claim.claim_generation=NEW.claim_generation
      AND NEW.lineage_id=claim.lineage_id AND NEW.generation=claim.generation
      AND NEW.remote_runner_id IS remote.runner_id AND NEW.remote_runner_name=claim.requested_name AND NEW.remote_scale_set_id=claim.scale_set_id
      AND current.lineage_id=claim.lineage_id AND current.generation=claim.generation AND current.reservation_id=claim.reservation_id AND current.intent_id=claim.intent_id
      AND attempt.physical_nonce=claim.physical_nonce AND attempt.binding_digest=claim.binding_digest AND attempt.source_digest=claim.source_digest AND (attempt.acquire_response_digest IS NULL OR attempt.acquire_response_digest=claim.acquire_response_digest) AND attempt.engine_id=claim.engine_id AND attempt.docker_root_digest=claim.docker_root_digest
      AND intent.state='uncertain' AND intent.launch_phase='jit_uncertain' AND intent.kind='launch' AND intent.cleanup_proven=0 AND intent.jit_requested=1 AND intent.github_runner_id IS NULL AND intent.runner_name=claim.requested_name
      AND jit.state='uncertain' AND jit.runner_id IS NULL AND jit.runner_name IS NULL AND jit.requested_name=claim.requested_name AND jit.started_request_id IS NULL AND jit.completed_request_id IS NULL
      AND state.resources_bound=1 AND state.storage_cleanup_proven=1 AND state.jit_state='uncertain'
      AND cleanup.instance_binding_digest=state.instance_binding_digest AND cleanup.role_mask=2047 AND cleanup.containers_absent=1 AND cleanup.volumes_absent=1 AND cleanup.mounts_detached=1 AND cleanup.cgroups_absent=1
      AND witness.engine_id=claim.engine_id AND witness.docker_root_digest=claim.docker_root_digest AND witness.instance_binding_digest=state.instance_binding_digest AND witness.runner_container_id=cleanup.runner_container_id AND witness.dind_container_id=cleanup.dind_container_id
      AND network.create_state='created' AND network.remove_state='removed' AND network.engine_id=claim.engine_id AND network.docker_root_digest=claim.docker_root_digest
      AND reservation.state='uncertain' AND reservation.remote_cleanup_proven=1 AND reservation.local_cleanup_proven=1
      AND sampled.outcome='settled' AND sampled.sample_generation_id=metadata.probe_operation_id AND metadata.connection_id=claim.connection_id AND metadata.scale_set_id=claim.scale_set_id AND metadata.operation_id=claim.intent_id AND metadata.engine_id=claim.engine_id AND metadata.docker_root_digest=claim.docker_root_digest
      AND length(local.evidence_sha256)=64 AND length(remote.evidence_sha256)=64 AND remote.lookup_status=200 AND remote.lookup_count=0 AND remote.request_sha256 IS NOT NULL AND remote.response_sha256 IS NOT NULL AND remote.runner_name=claim.requested_name AND remote.runner_scale_set_id=claim.scale_set_id AND NEW.proof_digest=NEW.settlement_digest
      AND ((remote.runner_id IS NULL AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress initial WHERE initial.reservation_id=claim.reservation_id AND initial.claim_generation=claim.claim_generation AND initial.phase_index=2 AND initial.phase='remote_initial_absent' AND initial.runner_id IS NULL AND initial.runner_name=claim.requested_name AND initial.runner_scale_set_id=claim.scale_set_id AND initial.lookup_status=200 AND initial.lookup_count=0 AND initial.request_sha256 IS NOT NULL AND initial.response_sha256 IS NOT NULL))
           OR (remote.runner_id IS NOT NULL AND (EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress target JOIN physical_attempt_jit_conflict_recovery_progress deletion ON deletion.reservation_id=target.reservation_id AND deletion.claim_generation=target.claim_generation AND deletion.phase_index=3 AND deletion.phase='delete_started' WHERE target.reservation_id=claim.reservation_id AND target.claim_generation=claim.claim_generation AND target.phase_index=2 AND target.phase='remote_target' AND target.runner_id=remote.runner_id AND target.runner_name=claim.requested_name AND target.runner_scale_set_id=claim.scale_set_id AND deletion.runner_id=remote.runner_id AND deletion.runner_name=claim.requested_name AND deletion.runner_scale_set_id=claim.scale_set_id)
               OR EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress target JOIN physical_attempt_jit_conflict_recovery_progress deletion ON deletion.reservation_id=target.reservation_id AND deletion.claim_generation=target.claim_generation AND deletion.phase_index=3 AND deletion.phase='delete_started' WHERE target.reservation_id=claim.reservation_id AND target.claim_generation<claim.claim_generation AND target.phase_index=2 AND target.phase='remote_target' AND target.runner_id=remote.runner_id AND target.runner_name=claim.requested_name AND target.runner_scale_set_id=claim.scale_set_id AND deletion.runner_id=remote.runner_id AND deletion.runner_name=claim.requested_name AND deletion.runner_scale_set_id=claim.scale_set_id AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress prior WHERE prior.reservation_id=claim.reservation_id AND prior.claim_generation<claim.claim_generation AND prior.phase_index=2 AND prior.phase='remote_initial_absent')))))
      AND auth.acquire_state='acquired' AND auth.binding_digest=claim.binding_digest AND auth.source_digest=claim.source_digest AND auth.acquire_response_digest=claim.acquire_response_digest AND auth.started_event_digest IS NULL AND auth.completed_event_digest IS NULL
      AND active.generation=claim.authority_generation AND active.policy_digest=claim.authority_digest AND policy.binding_digest=claim.binding_digest
      AND claim.lease_until>unixepoch() AND NEW.created_at=unixepoch()
      AND NOT EXISTS(SELECT 1 FROM connection_completion_inbox inbox WHERE inbox.connection_id=claim.connection_id AND inbox.scale_set_id=claim.scale_set_id)
      AND NOT EXISTS(SELECT 1 FROM connection_request_authorizations auth WHERE auth.connection_id=claim.connection_id AND auth.scale_set_id=claim.scale_set_id AND ((auth.started_event_digest IS NOT NULL AND auth.started_runner_name=claim.requested_name) OR (auth.completed_event_digest IS NOT NULL AND auth.completed_runner_name=claim.requested_name)))
)
BEGIN SELECT RAISE(ABORT,'The JIT conflict settlement proof is invalid.'); END";

pub(super) const SETTLEMENT_NO_UPDATE: &str = "CREATE TRIGGER physical_attempt_jit_conflict_settlement_no_update BEFORE UPDATE ON physical_attempt_jit_conflict_settlements BEGIN SELECT RAISE(ABORT,'JIT conflict settlement proofs are immutable.'); END";
pub(super) const SETTLEMENT_NO_DELETE: &str = "CREATE TRIGGER physical_attempt_jit_conflict_settlement_no_delete BEFORE DELETE ON physical_attempt_jit_conflict_settlements BEGIN SELECT RAISE(ABORT,'JIT conflict settlement proofs are immutable.'); END";

pub(super) const JIT_INSERT_FENCE: &str = "CREATE TRIGGER jit_conflict_jit_insert_fence BEFORE INSERT ON connection_runner_jits WHEN EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims claim WHERE claim.intent_id=NEW.intent_id AND claim.connection_id=NEW.connection_id AND claim.scale_set_id=NEW.scale_set_id) BEGIN SELECT RAISE(ABORT,'A JIT conflict claim prevents this operation.'); END";
pub(super) const JIT_START_FENCE: &str = "CREATE TRIGGER jit_conflict_jit_start_fence BEFORE UPDATE OF state ON connection_runner_jits WHEN NEW.state IN ('start_requested','start_uncertain','ready','started') AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims claim WHERE claim.intent_id=NEW.intent_id AND claim.connection_id=NEW.connection_id AND claim.scale_set_id=NEW.scale_set_id) BEGIN SELECT RAISE(ABORT,'A JIT conflict claim prevents this operation.'); END";
pub(super) const PROVISIONING_JIT_FENCE: &str = "CREATE TRIGGER jit_conflict_provisioning_jit_fence BEFORE UPDATE OF jit_state ON capacity_reservation_provisioning_state WHEN NEW.jit_state='requested' AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims claim WHERE claim.reservation_id=NEW.reservation_id) BEGIN SELECT RAISE(ABORT,'A JIT conflict claim prevents this operation.'); END";
pub(super) const PUBLICATION_FENCE: &str = "CREATE TRIGGER jit_conflict_publication_fence BEFORE UPDATE OF publication_state ON capacity_reservation_provisioning_state WHEN NEW.publication_state='requested' AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims claim WHERE claim.reservation_id=NEW.reservation_id) BEGIN SELECT RAISE(ABORT,'A JIT conflict claim prevents this operation.'); END";
pub(super) const DELIVERY_FENCE: &str = "CREATE TRIGGER jit_conflict_delivery_fence BEFORE UPDATE OF delivery_state ON capacity_reservation_provisioning_state WHEN NEW.delivery_state='requested' AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims claim WHERE claim.reservation_id=NEW.reservation_id) BEGIN SELECT RAISE(ABORT,'A JIT conflict claim prevents this operation.'); END";
pub(super) const CHALLENGE_FENCE: &str = "CREATE TRIGGER jit_conflict_challenge_fence BEFORE UPDATE OF challenge_state ON connection_execution_proofs WHEN NEW.challenge_state='requested' AND EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims claim WHERE claim.reservation_id=NEW.reservation_id AND claim.connection_id=NEW.connection_id AND claim.scale_set_id=NEW.scale_set_id) BEGIN SELECT RAISE(ABORT,'A JIT conflict claim prevents this operation.'); END";
pub(super) const DECISION_FENCE: &str = "CREATE TRIGGER jit_conflict_decision_fence BEFORE INSERT ON connection_execution_decisions WHEN EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims claim WHERE claim.reservation_id=NEW.reservation_id AND claim.connection_id=NEW.connection_id AND claim.scale_set_id=NEW.scale_set_id) BEGIN SELECT RAISE(ABORT,'A JIT conflict claim prevents this operation.'); END";
pub(super) const PUBLICATION_INSERT_FENCE: &str = "CREATE TRIGGER jit_conflict_publication_insert_fence BEFORE INSERT ON execution_publication_operations WHEN EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims claim WHERE claim.reservation_id=NEW.reservation_id AND claim.connection_id=NEW.connection_id AND claim.scale_set_id=NEW.scale_set_id) BEGIN SELECT RAISE(ABORT,'A JIT conflict claim prevents this operation.'); END";

pub(super) const IMMUTABLE_GUARDS: [&str; 6] = [
    CLAIM_NO_UPDATE,
    CLAIM_NO_DELETE,
    PROGRESS_NO_UPDATE,
    PROGRESS_NO_DELETE,
    SETTLEMENT_NO_UPDATE,
    SETTLEMENT_NO_DELETE,
];

pub(super) const INSERT_GUARDS: [&str; 7] = [
    CLAIM_INSERT_GUARD,
    PROGRESS_INSERT_GUARD,
    SETTLEMENT_INSERT_GUARD,
    METADATA_JIT_RECOVERY_PROOF_GUARD,
    METADATA_JIT_RECOVERY_DETAIL_PROOF_GUARD,
    METADATA_JIT_REMOTE_HISTORY_PROOF_GUARD,
    METADATA_JIT_REMOTE_NO_EVENTS_PROOF_GUARD,
];

pub(super) const EFFECT_FENCES: [&str; 8] = [
    JIT_INSERT_FENCE,
    JIT_START_FENCE,
    PROVISIONING_JIT_FENCE,
    PUBLICATION_FENCE,
    DELIVERY_FENCE,
    CHALLENGE_FENCE,
    DECISION_FENCE,
    PUBLICATION_INSERT_FENCE,
];

#[cfg(test)]
mod v40_metadata_guard_tests {
    use super::{
        LEGACY_METADATA_JIT_GATE, METADATA_JIT_RECOVERY_DETAIL_PROOF_GUARD,
        METADATA_JIT_RECOVERY_PROOF_GUARD, METADATA_JIT_REMOTE_HISTORY_PROOF_GUARD,
        METADATA_JIT_REMOTE_NO_EVENTS_PROOF_GUARD, V40_METADATA_JIT_GATE,
        metadata_claim_guard_sql_v40, replace_once,
    };

    #[test]
    fn v40_common_metadata_guard_changes_only_the_jit_eligibility_gate() {
        let legacy = super::super::super::provisioning::metadata_settlement_claim_guard_sql()
            .expect("metadata guard is part of the fixed provisioning contract");
        let expected = replace_once(legacy, LEGACY_METADATA_JIT_GATE, V40_METADATA_JIT_GATE)
            .expect("v40 state gate replacement occurs exactly once");
        let actual = metadata_claim_guard_sql_v40();
        assert!(actual.as_ref().is_ok_and(|actual| actual == &expected));
        assert!(expected.contains("s.jit_state IN ('not_requested', 'cleaned', 'uncertain')"));
        assert!(!expected.contains("physical_attempt_jit_conflict_recovery_claims"));
    }

    #[test]
    fn v40_uncertain_proof_guards_are_independent_reject_only_triggers() {
        for (guard, name) in [
            (
                METADATA_JIT_RECOVERY_PROOF_GUARD,
                "jit_conflict_metadata_claim_proof_guard_recovery_core",
            ),
            (
                METADATA_JIT_RECOVERY_DETAIL_PROOF_GUARD,
                "jit_conflict_metadata_claim_proof_guard_recovery_detail",
            ),
            (
                METADATA_JIT_REMOTE_HISTORY_PROOF_GUARD,
                "jit_conflict_metadata_claim_proof_guard_remote_history",
            ),
            (
                METADATA_JIT_REMOTE_NO_EVENTS_PROOF_GUARD,
                "jit_conflict_metadata_claim_proof_guard_remote_no_events",
            ),
        ] {
            assert!(guard.starts_with(&format!(
                "CREATE TRIGGER {name} BEFORE INSERT ON capacity_reservation_metadata_settlement_claims"
            )));
            assert!(guard.contains("candidate.jit_state='uncertain'"));
            assert!(guard.contains("AND NOT EXISTS("));
            assert!(guard.contains("r.id=NEW.reservation_id"));
            assert!(guard.contains("i.id=NEW.operation_id"));
            assert!(
                guard.contains("conflict.claim_generation=(SELECT MAX(latest.claim_generation)")
            );
            assert!(
                guard.ends_with(
                    "BEGIN SELECT RAISE(ABORT,'invalid metadata settlement claim'); END"
                )
            );
        }
    }
}
