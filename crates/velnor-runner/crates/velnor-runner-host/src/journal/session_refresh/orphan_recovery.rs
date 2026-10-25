//! Mark exact interrupted pre-execution runners for post-JIT recovery.

use crate::error::HostError;

const MARK_INTERRUPTED_READY_RESERVATIONS_SQL: &str = r"UPDATE capacity_reservations SET state='recovering'
WHERE state IN ('reserved','uncertain','recovering')
  AND remote_cleanup_proven=0 AND local_cleanup_proven=0
  AND EXISTS (
    SELECT 1 FROM physical_attempt_current cur
    JOIN physical_attempts a ON a.lineage_id=cur.lineage_id AND a.generation=cur.generation
      AND a.reservation_id=cur.reservation_id AND a.intent_id=cur.intent_id
    JOIN physical_attempt_lineages lineage ON lineage.lineage_id=a.lineage_id
    JOIN capacity_reservations r ON r.id=a.reservation_id AND r.intent_id=a.intent_id
      AND r.connection_id=a.connection_id
    JOIN connection_intents i ON i.id=a.intent_id AND i.connection_id=a.connection_id
    JOIN capacity_reservation_provisioning p ON p.reservation_id=r.id AND p.operation_id=i.id
    JOIN capacity_reservation_provisioning_state s ON s.reservation_id=r.id
    JOIN connection_runner_jits j ON j.intent_id=i.id AND j.connection_id=i.connection_id
    JOIN connection_request_authorizations auth ON auth.connection_id=a.connection_id
      AND auth.scale_set_id=a.scale_set_id AND auth.request_id=a.witness_request_id
    JOIN connection_runtime_bindings binding ON binding.connection_id=a.connection_id
    JOIN connection_execution_proofs proof ON proof.reservation_id=r.id
      AND proof.intent_id=i.id AND proof.connection_id=a.connection_id
    WHERE r.id=capacity_reservations.id AND cur.reservation_id=r.id
      AND a.lineage_id=cur.lineage_id AND a.generation=cur.generation
      AND a.reservation_id=cur.reservation_id AND a.intent_id=cur.intent_id
      AND a.connection_id=r.connection_id AND a.scale_set_id=i.scale_set_id
      AND a.witness_request_id=j.witness_request_id
      AND lineage.connection_id=a.connection_id AND lineage.scale_set_id=a.scale_set_id
      AND lineage.binding_digest=a.binding_digest
      AND r.state IN ('reserved','uncertain','recovering')
      AND r.binding_digest=a.binding_digest AND r.engine_id=a.engine_id
      AND r.docker_root_digest=a.docker_root_digest
      AND i.kind='launch' AND i.scale_set_id=a.scale_set_id
      AND i.runner_name=j.runner_name AND i.github_runner_id=CAST(j.runner_id AS TEXT)
      AND i.cleanup_proven=0 AND i.runner_completed=0 AND i.runner_request_id IS NULL
      AND i.assignment_key=r.assignment_key
      AND p.engine_id=a.engine_id AND p.docker_root_digest=a.docker_root_digest
      AND binding.scale_set_id=a.scale_set_id
      AND auth.binding_digest=a.binding_digest AND auth.source_digest=a.source_digest
      AND auth.acquire_response_digest IS NOT NULL AND auth.acquire_state='acquired'
      AND auth.acquire_response_json IS NOT NULL
      AND (a.acquire_response_digest IS NULL OR a.acquire_response_digest=auth.acquire_response_digest)
      AND s.instance_binding_digest=proof.instance_binding_digest
      AND s.runner_id=j.runner_id AND s.runner_name=j.runner_name
      AND s.runner_scale_set_id=a.scale_set_id
      AND s.bootstrap_start_state='started' AND s.resources_bound=1
      AND s.storage_cleanup_proven=0 AND s.jit_state='bound'
      AND s.publication_state='published' AND s.delivery_state='delivered'
      AND j.state='ready' AND j.started_request_id IS NULL AND j.completed_request_id IS NULL
      AND j.scale_set_id=a.scale_set_id AND j.requested_name=j.runner_name
      AND proof.scale_set_id=a.scale_set_id AND proof.witness_request_id=a.witness_request_id
      AND proof.binding_digest=a.binding_digest AND proof.engine_id=a.engine_id
      AND proof.docker_root_digest=a.docker_root_digest
      AND proof.runner_id=j.runner_id AND proof.runner_name=j.runner_name
      AND proof.proof_state='denied' AND proof.deny_code='interrupted_after_restart'
      AND proof.challenge_state='abandoned' AND proof.actual_request_id IS NULL
      AND proof.started_event_digest IS NULL
      AND proof.challenge_file_path='/opt/velnor/source-proof/control/challenge.json'
      AND proof.challenge_file_inode IS NOT NULL
      AND proof.challenge_file_bytes=length(CAST(proof.challenge_json AS BLOB))
      AND proof.challenge_file_uid=0 AND proof.challenge_file_gid=0
      AND proof.challenge_file_mode=292 AND proof.challenge_mount_id IS NOT NULL
      AND proof.challenge_device_major IS NOT NULL AND proof.challenge_device_minor IS NOT NULL
      AND proof.challenge_statx_device_major IS NOT NULL
      AND proof.challenge_statx_device_minor IS NOT NULL
      AND proof.challenge_fs_type_magic IS NOT NULL
      AND proof.challenge_filesystem_id IS NOT NULL
      AND proof.challenge_filesystem_instance_digest IS NOT NULL
      AND proof.challenge_read_only=1 AND proof.challenge_no_copy=1
      AND NOT EXISTS (SELECT 1 FROM physical_attempt_outcomes outcome
        WHERE outcome.lineage_id=a.lineage_id AND outcome.generation=a.generation
          AND outcome.reservation_id=a.reservation_id)
      AND NOT EXISTS (SELECT 1 FROM physical_attempt_post_jit_retirements retired
        WHERE retired.lineage_id=a.lineage_id AND retired.generation=a.generation
          AND retired.reservation_id=a.reservation_id)
      AND NOT EXISTS (SELECT 1 FROM physical_attempt_post_jit_retirement_claims claim
        WHERE claim.reservation_id=a.reservation_id)
      AND j.runner_id>0 AND j.runner_name='v'||a.physical_nonce
      AND EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers bc
        WHERE bc.reservation_id=r.id AND bc.role='runner' AND bc.create_state='created'
          AND bc.start_state='started' AND bc.container_id=s.runner_container_id)
      AND EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers bc
        WHERE bc.reservation_id=r.id AND bc.role='dind' AND bc.create_state='created'
          AND bc.start_state='started' AND bc.container_id=s.dind_container_id)
      AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_objects obj
        WHERE obj.reservation_id=r.id AND obj.create_state!='created')
      AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects obj
        WHERE obj.reservation_id=r.id AND obj.create_state='created')=5
      AND EXISTS (SELECT 1 FROM capacity_reservation_networks net
        WHERE net.reservation_id=r.id AND net.connection_id=a.connection_id
          AND net.operation_id=i.id AND net.engine_id=a.engine_id
          AND net.docker_root_digest=a.docker_root_digest AND net.create_state='created'
          AND net.network_id IS NOT NULL AND net.remove_state='not_requested')
      AND EXISTS (SELECT 1 FROM capacity_reservation_storage_initialization init
        JOIN capacity_reservation_storage_handoffs handoff
          ON handoff.reservation_id=init.reservation_id
          AND handoff.connection_id=init.connection_id
          AND handoff.operation_id=init.operation_id
          AND handoff.generation=init.generation
          AND handoff.helper_name=init.helper_name AND handoff.helper_id=init.helper_id
          AND handoff.engine_id=init.engine_id
          AND handoff.docker_root_digest=init.docker_root_digest
          AND handoff.metadata_volume_name=init.metadata_volume
          AND handoff.metadata_volume_id=init.metadata_volume_id
          AND handoff.operation_key=init.operation_key
          AND handoff.layout_digest=init.metadata_layout_digest
        WHERE init.reservation_id=r.id AND init.connection_id=a.connection_id
          AND init.operation_id=i.id AND init.engine_id=a.engine_id
          AND init.docker_root_digest=a.docker_root_digest
          AND init.create_dispatch_state='response_received' AND init.create_state='created'
          AND init.start_state='started' AND init.result_state='confirmed'
          AND init.remove_state='removed' AND init.abort_state='not_requested'
          AND init.abort_lease_until=0 AND init.layout_version=2
          AND handoff.state='attached'
          AND handoff.runner_container_id=s.runner_container_id
          AND handoff.dind_container_id=s.dind_container_id)
      AND NOT EXISTS (SELECT 1 FROM connection_execution_decisions decision
        WHERE decision.intent_id=i.id)
      AND NOT EXISTS (SELECT 1 FROM connection_request_authorizations actual
        WHERE actual.connection_id=a.connection_id AND actual.scale_set_id=a.scale_set_id
          AND ((actual.started_event_digest IS NOT NULL AND actual.started_runner_id=j.runner_id
                AND actual.started_runner_name=j.runner_name)
            OR (actual.completed_event_digest IS NOT NULL AND actual.completed_runner_id=j.runner_id
                AND actual.completed_runner_name=j.runner_name)))
      AND NOT EXISTS (SELECT 1 FROM execution_publication_operations publication
        JOIN resource_probe_operations op ON op.operation_id=publication.operation_id
        WHERE publication.reservation_id=r.id AND op.phase NOT IN ('removed','aborted'))
  )";

pub(super) async fn mark_interrupted_ready_reservations(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    connection
        .execute(MARK_INTERRUPTED_READY_RESERVATIONS_SQL, ())
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)
}
