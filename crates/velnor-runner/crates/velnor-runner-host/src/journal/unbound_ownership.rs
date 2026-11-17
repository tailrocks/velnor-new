// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) const CLASSIFICATION: &str = r"SELECT r.id, r.connection_id FROM physical_attempt_current cur
CROSS INNER JOIN physical_attempts a ON a.lineage_id=cur.lineage_id AND a.generation=cur.generation AND a.reservation_id=cur.reservation_id AND a.intent_id=cur.intent_id
CROSS INNER JOIN physical_attempt_lineages lineage ON lineage.lineage_id=a.lineage_id AND lineage.connection_id=a.connection_id AND lineage.scale_set_id=a.scale_set_id AND lineage.binding_digest=a.binding_digest
CROSS INNER JOIN capacity_reservations r ON r.id=a.reservation_id AND r.intent_id=a.intent_id AND r.connection_id=a.connection_id AND r.binding_digest=a.binding_digest AND r.engine_id=a.engine_id AND r.docker_root_digest=a.docker_root_digest
CROSS INNER JOIN connection_intents i ON i.id=a.intent_id AND i.connection_id=a.connection_id AND i.scale_set_id=a.scale_set_id AND i.assignment_key=r.assignment_key
CROSS INNER JOIN connection_scopes scope ON scope.connection_id=a.connection_id AND scope.retired=0
CROSS INNER JOIN connection_runtime_bindings binding ON binding.connection_id=a.connection_id AND binding.scale_set_id=a.scale_set_id AND binding.engine_id=a.engine_id
CROSS INNER JOIN capacity_reservation_provisioning p ON p.reservation_id=r.id AND p.operation_id=i.id AND p.engine_id=a.engine_id AND p.docker_root_digest=a.docker_root_digest
CROSS INNER JOIN capacity_reservation_provisioning_topology topology ON topology.reservation_id=r.id AND topology.topology_version=2
CROSS INNER JOIN capacity_reservation_provisioning_state s ON s.reservation_id=r.id
WHERE r.remote_cleanup_proven=0 AND r.local_cleanup_proven=0 AND i.kind='launch' AND i.cleanup_proven=0 AND i.jit_requested=0 AND i.github_runner_id IS NULL
AND ((r.state='reserved' AND i.state='pending') OR (r.state='uncertain' AND i.state='uncertain'))
AND s.resources_bound=0 AND s.jit_state='not_requested' AND s.publication_state='not_requested' AND s.delivery_state='not_requested' AND s.storage_cleanup_proven=0
AND NOT EXISTS(SELECT 1 FROM connection_runner_jits jit WHERE jit.intent_id=i.id)
AND NOT EXISTS(SELECT 1 FROM connection_execution_proofs proof WHERE proof.intent_id=i.id)
AND NOT EXISTS(SELECT 1 FROM connection_execution_decisions decision WHERE decision.intent_id=i.id)
AND NOT EXISTS(SELECT 1 FROM execution_publication_operations publication WHERE publication.reservation_id=r.id)
AND NOT EXISTS(SELECT 1 FROM capacity_reservation_pre_jit_cleanup_claims owner WHERE owner.reservation_id=r.id)
AND NOT EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_claims owner WHERE owner.reservation_id=r.id)
AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims owner WHERE owner.reservation_id=r.id)
AND NOT EXISTS(SELECT 1 FROM connection_request_authorizations auth WHERE auth.connection_id=a.connection_id AND auth.scale_set_id=a.scale_set_id AND ((auth.started_event_digest IS NOT NULL AND auth.started_runner_name=i.runner_name) OR (auth.completed_event_digest IS NOT NULL AND auth.completed_runner_name=i.runner_name)))";

pub(super) fn base_sql() -> String {
    format!(
        "{CLASSIFICATION} AND NOT EXISTS(SELECT 1 FROM connection_completion_inbox inbox WHERE inbox.connection_id=a.connection_id AND inbox.scale_set_id=a.scale_set_id)"
    )
}
