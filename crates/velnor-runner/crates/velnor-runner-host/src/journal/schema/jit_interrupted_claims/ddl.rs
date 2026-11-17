// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) const TABLE: &str = r"CREATE TABLE physical_attempt_jit_interrupted_claims (
    lineage_id INTEGER NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    reservation_id INTEGER NOT NULL UNIQUE,
    intent_id INTEGER NOT NULL,
    connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id),
    scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0),
    witness_request_id INTEGER NOT NULL CHECK (witness_request_id > 0),
    binding_digest TEXT NOT NULL CHECK (length(binding_digest)=64 AND binding_digest NOT GLOB '*[^0-9a-f]*'),
    source_digest TEXT NOT NULL CHECK (length(source_digest)=64 AND source_digest NOT GLOB '*[^0-9a-f]*'),
    acquire_response_digest TEXT NOT NULL CHECK (length(acquire_response_digest)=64 AND acquire_response_digest NOT GLOB '*[^0-9a-f]*'),
    authority_generation INTEGER NOT NULL CHECK (authority_generation>0),
    authority_digest TEXT NOT NULL CHECK (length(authority_digest)=64 AND authority_digest NOT GLOB '*[^0-9a-f]*'),
    engine_id TEXT NOT NULL CHECK (length(CAST(engine_id AS BLOB)) BETWEEN 1 AND 256),
    docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest)=64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'),
    requested_name TEXT NOT NULL CHECK (length(CAST(requested_name AS BLOB)) BETWEEN 1 AND 128),
    created_at INTEGER NOT NULL CHECK (created_at>=0),
    PRIMARY KEY (lineage_id,generation),
    FOREIGN KEY (lineage_id,generation,reservation_id) REFERENCES physical_attempts(lineage_id,generation,reservation_id),
    FOREIGN KEY (lineage_id,generation,intent_id) REFERENCES physical_attempts(lineage_id,generation,intent_id),
    FOREIGN KEY (connection_id,scale_set_id,witness_request_id) REFERENCES connection_request_authorizations(connection_id,scale_set_id,request_id),
    FOREIGN KEY (connection_id,authority_generation,authority_digest) REFERENCES connection_job_authority_policies(connection_id,generation,policy_digest)
)";

pub(super) const COLUMNS: &str = "lineage_id,generation,reservation_id,intent_id,connection_id,scale_set_id,witness_request_id,binding_digest,source_digest,acquire_response_digest,authority_generation,authority_digest,engine_id,docker_root_digest,requested_name,created_at";

pub(super) const ELIGIBLE: &str = r"SELECT attempt.lineage_id,attempt.generation,attempt.reservation_id,attempt.intent_id,attempt.connection_id,attempt.scale_set_id,attempt.witness_request_id,attempt.binding_digest,attempt.source_digest,auth.acquire_response_digest,active.generation AS authority_generation,active.policy_digest AS authority_digest,attempt.engine_id,attempt.docker_root_digest,jit.requested_name,unixepoch() AS created_at
FROM physical_attempt_current cur
JOIN physical_attempts attempt ON attempt.lineage_id=cur.lineage_id AND attempt.generation=cur.generation AND attempt.reservation_id=cur.reservation_id AND attempt.intent_id=cur.intent_id
JOIN capacity_reservations reservation ON reservation.id=attempt.reservation_id AND reservation.intent_id=attempt.intent_id AND reservation.connection_id=attempt.connection_id
JOIN connection_intents intent ON intent.id=attempt.intent_id AND intent.connection_id=attempt.connection_id
JOIN capacity_reservation_provisioning provisioning ON provisioning.reservation_id=reservation.id AND provisioning.operation_id=attempt.intent_id AND provisioning.engine_id=attempt.engine_id AND provisioning.docker_root_digest=attempt.docker_root_digest
JOIN capacity_reservation_provisioning_state state ON state.reservation_id=reservation.id
JOIN connection_runner_jits jit ON jit.intent_id=attempt.intent_id AND jit.connection_id=attempt.connection_id
JOIN connection_request_authorizations auth ON auth.connection_id=attempt.connection_id AND auth.scale_set_id=attempt.scale_set_id AND auth.request_id=attempt.witness_request_id
JOIN connection_active_job_authority active ON active.connection_id=attempt.connection_id
JOIN connection_job_authority_policies policy ON policy.connection_id=active.connection_id AND policy.generation=active.generation AND policy.policy_digest=active.policy_digest
WHERE reservation.state='reserved' AND reservation.remote_cleanup_proven=0 AND reservation.local_cleanup_proven=0
AND reservation.binding_digest=attempt.binding_digest AND reservation.engine_id=attempt.engine_id AND reservation.docker_root_digest=attempt.docker_root_digest
AND intent.kind='launch' AND intent.scale_set_id=attempt.scale_set_id AND intent.runner_name=jit.requested_name AND intent.state='pending' AND intent.cleanup_proven=0 AND intent.jit_requested=1 AND intent.github_runner_id IS NULL AND intent.launch_phase='jit_requested' AND intent.runner_request_id IS NULL AND intent.runner_completed=0
AND state.resources_bound=1 AND state.bootstrap_start_state='started' AND state.storage_cleanup_proven=0 AND state.jit_state='requested' AND state.runner_id IS NULL AND state.runner_name IS NULL AND state.runner_scale_set_id IS NULL AND state.publication_state='not_requested' AND state.delivery_state='not_requested'
AND jit.scale_set_id=attempt.scale_set_id AND jit.witness_request_id=attempt.witness_request_id AND jit.requested_name='v'||attempt.physical_nonce AND jit.state='claimed' AND jit.runner_id IS NULL AND jit.runner_name IS NULL AND jit.started_request_id IS NULL AND jit.completed_request_id IS NULL
AND auth.binding_digest=attempt.binding_digest AND auth.source_digest=attempt.source_digest AND (attempt.acquire_response_digest IS NULL OR attempt.acquire_response_digest=auth.acquire_response_digest) AND auth.acquire_response_digest IS NOT NULL AND auth.acquire_state='acquired' AND auth.started_event_digest IS NULL AND auth.completed_event_digest IS NULL AND policy.binding_digest=attempt.binding_digest
AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_receipts receipt WHERE receipt.reservation_id=reservation.id)
AND NOT EXISTS(SELECT 1 FROM physical_attempt_jit_interrupted_claims interrupted WHERE interrupted.reservation_id=reservation.id)
AND NOT EXISTS(SELECT 1 FROM connection_completion_inbox inbox WHERE inbox.connection_id=attempt.connection_id AND inbox.scale_set_id=attempt.scale_set_id)
AND NOT EXISTS(SELECT 1 FROM connection_request_authorizations event_auth WHERE event_auth.connection_id=attempt.connection_id AND event_auth.scale_set_id=attempt.scale_set_id AND ((event_auth.started_event_digest IS NOT NULL AND event_auth.started_runner_name=jit.requested_name) OR (event_auth.completed_event_digest IS NOT NULL AND event_auth.completed_runner_name=jit.requested_name)))
AND NOT EXISTS(SELECT 1 FROM connection_execution_decisions decision WHERE decision.intent_id=attempt.intent_id)
AND NOT EXISTS(SELECT 1 FROM execution_publication_operations publication WHERE publication.reservation_id=reservation.id)
AND NOT EXISTS(SELECT 1 FROM connection_execution_proofs proof WHERE proof.reservation_id=reservation.id)";

pub(super) const RECEIPT_FENCE: &str = "CREATE TRIGGER physical_attempt_jit_conflict_receipt_interrupted_fence BEFORE INSERT ON physical_attempt_jit_conflict_receipts WHEN EXISTS(SELECT 1 FROM physical_attempt_jit_interrupted_claims interrupted WHERE interrupted.reservation_id=NEW.reservation_id) BEGIN SELECT RAISE(ABORT,'Interrupted JIT claims cannot become HTTP conflict receipts.'); END";

pub(super) fn objects() -> Vec<(&'static str, String, String)> {
    let mut objects = vec![(
        "table",
        "physical_attempt_jit_interrupted_claims".to_owned(),
        TABLE.to_owned(),
    )];
    objects.push(("view", "physical_attempt_jit_recovery_sources".to_owned(), format!("CREATE VIEW physical_attempt_jit_recovery_sources AS SELECT {COLUMNS},'conflict' AS source_kind FROM physical_attempt_jit_conflict_receipts UNION ALL SELECT {COLUMNS},'interrupted' AS source_kind FROM physical_attempt_jit_interrupted_claims")));
    let matches = COLUMNS
        .split(',')
        .map(|column| format!("candidate.{column}=NEW.{column}"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let guard = format!(
        "CREATE TRIGGER physical_attempt_jit_interrupted_claim_insert_guard BEFORE INSERT ON physical_attempt_jit_interrupted_claims WHEN NOT EXISTS(SELECT 1 FROM ({ELIGIBLE}) candidate WHERE {matches}) BEGIN SELECT RAISE(ABORT,'The interrupted JIT claim does not match the current acquired physical attempt.'); END"
    );
    objects.push((
        "trigger",
        "physical_attempt_jit_interrupted_claim_insert_guard".to_owned(),
        guard,
    ));
    objects.push((
        "trigger",
        "physical_attempt_jit_conflict_receipt_interrupted_fence".to_owned(),
        RECEIPT_FENCE.to_owned(),
    ));
    for action in ["update", "delete"] {
        let name = format!("physical_attempt_jit_interrupted_claim_no_{action}");
        objects.push(("trigger", name.clone(), format!("CREATE TRIGGER {name} BEFORE {action} ON physical_attempt_jit_interrupted_claims BEGIN SELECT RAISE(ABORT,'Interrupted JIT claims are immutable.'); END")));
    }
    for (suffix, action) in [
        ("insert", "AFTER INSERT"),
        ("update", "AFTER UPDATE"),
        ("delete", "AFTER DELETE"),
    ] {
        let table = "physical_attempt_jit_interrupted_claims";
        objects.push((
            "trigger",
            format!("{table}_revision_{suffix}"),
            super::super::jit_conflict_settlement::revision_trigger_sql(table, suffix, action),
        ));
    }
    objects
}
