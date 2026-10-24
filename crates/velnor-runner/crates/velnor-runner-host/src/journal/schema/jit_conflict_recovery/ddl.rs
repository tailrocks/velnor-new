//! Immutable receipt objects for definite JIT conflict responses.

pub(super) const TABLE: &str = r"CREATE TABLE IF NOT EXISTS physical_attempt_jit_conflict_receipts (
    lineage_id INTEGER NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    reservation_id INTEGER NOT NULL,
    intent_id INTEGER NOT NULL,
    connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id),
    scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0),
    witness_request_id INTEGER NOT NULL CHECK (witness_request_id > 0),
    binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'),
    source_digest TEXT NOT NULL CHECK (length(source_digest) = 64 AND source_digest NOT GLOB '*[^0-9a-f]*'),
    acquire_response_digest TEXT NOT NULL CHECK (length(acquire_response_digest) = 64 AND acquire_response_digest NOT GLOB '*[^0-9a-f]*'),
    authority_generation INTEGER NOT NULL CHECK (authority_generation > 0),
    authority_digest TEXT NOT NULL CHECK (length(authority_digest) = 64 AND authority_digest NOT GLOB '*[^0-9a-f]*'),
    engine_id TEXT NOT NULL CHECK (length(CAST(engine_id AS BLOB)) BETWEEN 1 AND 256),
    docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'),
    requested_name TEXT NOT NULL CHECK (length(CAST(requested_name AS BLOB)) BETWEEN 1 AND 128),
    response_status INTEGER NOT NULL CHECK (response_status = 409),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    PRIMARY KEY (lineage_id, generation),
    UNIQUE (reservation_id),
    FOREIGN KEY (lineage_id, generation, reservation_id) REFERENCES physical_attempts(lineage_id, generation, reservation_id),
    FOREIGN KEY (lineage_id, generation, intent_id) REFERENCES physical_attempts(lineage_id, generation, intent_id),
    FOREIGN KEY (connection_id, scale_set_id, witness_request_id) REFERENCES connection_request_authorizations(connection_id, scale_set_id, request_id),
    FOREIGN KEY (connection_id, authority_generation, authority_digest) REFERENCES connection_job_authority_policies(connection_id, generation, policy_digest)
)";

pub(super) const WITNESS_INDEX: &str = "CREATE INDEX IF NOT EXISTS physical_attempt_jit_conflict_receipts_witness ON physical_attempt_jit_conflict_receipts(connection_id, scale_set_id, witness_request_id)";

pub(super) const INSERT_GUARD: &str = r"CREATE TRIGGER physical_attempt_jit_conflict_receipt_insert_guard BEFORE INSERT ON physical_attempt_jit_conflict_receipts
WHEN NOT EXISTS (
    SELECT 1
    FROM physical_attempt_current cur
    JOIN physical_attempts attempt ON attempt.lineage_id=cur.lineage_id AND attempt.generation=cur.generation AND attempt.reservation_id=cur.reservation_id AND attempt.intent_id=cur.intent_id
    JOIN capacity_reservations reservation ON reservation.id=attempt.reservation_id AND reservation.intent_id=attempt.intent_id AND reservation.connection_id=attempt.connection_id
    JOIN connection_intents intent ON intent.id=attempt.intent_id AND intent.connection_id=attempt.connection_id
    JOIN capacity_reservation_provisioning_state state ON state.reservation_id=reservation.id
    JOIN connection_runner_jits jit ON jit.intent_id=attempt.intent_id AND jit.connection_id=attempt.connection_id
    JOIN connection_request_authorizations auth ON auth.connection_id=attempt.connection_id AND auth.scale_set_id=attempt.scale_set_id AND auth.request_id=attempt.witness_request_id
    JOIN connection_active_job_authority active ON active.connection_id=attempt.connection_id
    JOIN connection_job_authority_policies policy ON policy.connection_id=active.connection_id AND policy.generation=active.generation AND policy.policy_digest=active.policy_digest
    WHERE cur.lineage_id=NEW.lineage_id AND cur.generation=NEW.generation AND cur.reservation_id=NEW.reservation_id AND cur.intent_id=NEW.intent_id
      AND attempt.lineage_id=NEW.lineage_id AND attempt.generation=NEW.generation AND attempt.reservation_id=NEW.reservation_id AND attempt.intent_id=NEW.intent_id
      AND attempt.connection_id=NEW.connection_id AND attempt.scale_set_id=NEW.scale_set_id AND attempt.witness_request_id=NEW.witness_request_id
      AND attempt.binding_digest=NEW.binding_digest AND attempt.source_digest=NEW.source_digest AND (attempt.acquire_response_digest IS NULL OR attempt.acquire_response_digest=NEW.acquire_response_digest)
      AND attempt.engine_id=NEW.engine_id AND attempt.docker_root_digest=NEW.docker_root_digest
      AND intent.kind='launch' AND intent.scale_set_id=NEW.scale_set_id AND intent.runner_name=NEW.requested_name AND intent.state='pending' AND intent.cleanup_proven=0 AND intent.jit_requested=1 AND intent.github_runner_id IS NULL AND intent.launch_phase='jit_requested'
      AND reservation.state='reserved' AND reservation.remote_cleanup_proven=0 AND reservation.local_cleanup_proven=0 AND reservation.binding_digest=NEW.binding_digest AND reservation.engine_id=NEW.engine_id AND reservation.docker_root_digest=NEW.docker_root_digest
      AND state.jit_state='requested' AND state.runner_id IS NULL AND state.runner_name IS NULL AND state.runner_scale_set_id IS NULL
      AND jit.scale_set_id=NEW.scale_set_id AND jit.witness_request_id=NEW.witness_request_id AND jit.requested_name=NEW.requested_name AND jit.state='claimed' AND jit.runner_id IS NULL AND jit.runner_name IS NULL
      AND NEW.requested_name='v'||attempt.physical_nonce
      AND auth.binding_digest=NEW.binding_digest AND auth.source_digest=NEW.source_digest AND auth.acquire_response_digest=NEW.acquire_response_digest AND auth.acquire_state='acquired'
      AND active.generation=NEW.authority_generation AND active.policy_digest=NEW.authority_digest AND policy.binding_digest=NEW.binding_digest
      AND NEW.response_status=409 AND NEW.created_at=unixepoch()
)
BEGIN SELECT RAISE(ABORT,'The JIT conflict receipt does not match the current acquired physical attempt.'); END";

pub(super) const NO_UPDATE: &str = "CREATE TRIGGER physical_attempt_jit_conflict_receipt_no_update BEFORE UPDATE ON physical_attempt_jit_conflict_receipts BEGIN SELECT RAISE(ABORT,'JIT conflict receipts are immutable.'); END";
pub(super) const NO_DELETE: &str = "CREATE TRIGGER physical_attempt_jit_conflict_receipt_no_delete BEFORE DELETE ON physical_attempt_jit_conflict_receipts BEGIN SELECT RAISE(ABORT,'JIT conflict receipts are immutable.'); END";
