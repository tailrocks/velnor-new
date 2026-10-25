//! Version-40 JIT conflict recovery storage and effect fences.

pub(super) const JIT_TABLE: &str = r"CREATE TABLE connection_runner_jits (
    intent_id INTEGER PRIMARY KEY REFERENCES connection_intents(id),
    connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id),
    scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0),
    witness_request_id INTEGER NOT NULL CHECK (witness_request_id > 0),
    requested_name TEXT NOT NULL CHECK (length(CAST(requested_name AS BLOB)) BETWEEN 1 AND 128),
    state TEXT NOT NULL CHECK (state IN ('claimed', 'uncertain', 'bound', 'start_requested', 'start_uncertain', 'ready', 'started', 'completed', 'cleaned', 'conflict_retired')),
    runner_id INTEGER CHECK (runner_id IS NULL OR runner_id > 0),
    runner_name TEXT CHECK (runner_name IS NULL OR length(CAST(runner_name AS BLOB)) BETWEEN 1 AND 128),
    started_request_id INTEGER CHECK (started_request_id IS NULL OR started_request_id > 0),
    completed_request_id INTEGER CHECK (completed_request_id IS NULL OR completed_request_id > 0),
    FOREIGN KEY (connection_id, scale_set_id, witness_request_id) REFERENCES connection_request_authorizations(connection_id, scale_set_id, request_id),
    FOREIGN KEY (connection_id, scale_set_id, started_request_id) REFERENCES connection_request_authorizations(connection_id, scale_set_id, request_id),
    FOREIGN KEY (connection_id, scale_set_id, completed_request_id) REFERENCES connection_request_authorizations(connection_id, scale_set_id, request_id),
    CHECK ((state IN ('claimed', 'uncertain', 'conflict_retired') AND runner_id IS NULL AND runner_name IS NULL) OR (state NOT IN ('claimed', 'uncertain', 'conflict_retired') AND runner_id IS NOT NULL AND runner_name IS NOT NULL)),
    CHECK (state != 'started' OR started_request_id IS NOT NULL),
    CHECK (state != 'completed' OR completed_request_id IS NOT NULL)
)";

pub(super) const CLAIMS: &str = r"CREATE TABLE physical_attempt_jit_conflict_recovery_claims (
    reservation_id INTEGER NOT NULL,
    claim_generation INTEGER NOT NULL CHECK (claim_generation > 0),
    lineage_id INTEGER NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    intent_id INTEGER NOT NULL,
    connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id),
    scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0),
    witness_request_id INTEGER NOT NULL CHECK (witness_request_id > 0),
    physical_nonce TEXT NOT NULL CHECK (length(physical_nonce) = 32 AND physical_nonce NOT GLOB '*[^0-9a-f]*'),
    binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'),
    source_digest TEXT NOT NULL CHECK (length(source_digest) = 64 AND source_digest NOT GLOB '*[^0-9a-f]*'),
    acquire_response_digest TEXT NOT NULL CHECK (length(acquire_response_digest) = 64 AND acquire_response_digest NOT GLOB '*[^0-9a-f]*'),
    authority_generation INTEGER NOT NULL CHECK (authority_generation > 0),
    authority_digest TEXT NOT NULL CHECK (length(authority_digest) = 64 AND authority_digest NOT GLOB '*[^0-9a-f]*'),
    engine_id TEXT NOT NULL CHECK (length(CAST(engine_id AS BLOB)) BETWEEN 1 AND 256),
    docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'),
    requested_name TEXT NOT NULL CHECK (length(CAST(requested_name AS BLOB)) BETWEEN 1 AND 128),
    snapshot_sha256 TEXT NOT NULL CHECK (length(snapshot_sha256) = 64 AND snapshot_sha256 NOT GLOB '*[^0-9a-f]*'),
    lease_until INTEGER NOT NULL CHECK (lease_until >= 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    PRIMARY KEY (reservation_id, claim_generation),
    FOREIGN KEY (lineage_id, generation, reservation_id) REFERENCES physical_attempts(lineage_id, generation, reservation_id),
    FOREIGN KEY (lineage_id, generation, intent_id) REFERENCES physical_attempts(lineage_id, generation, intent_id),
    FOREIGN KEY (connection_id, scale_set_id, witness_request_id) REFERENCES connection_request_authorizations(connection_id, scale_set_id, request_id)
)";

pub(super) const PROGRESS: &str = r"CREATE TABLE physical_attempt_jit_conflict_recovery_progress (
    reservation_id INTEGER NOT NULL,
    claim_generation INTEGER NOT NULL CHECK (claim_generation > 0),
    phase_index INTEGER NOT NULL CHECK (phase_index BETWEEN 1 AND 4),
    phase TEXT NOT NULL CHECK ((phase_index = 1 AND phase = 'local_absent') OR (phase_index = 2 AND phase IN ('remote_target','remote_initial_absent')) OR (phase_index = 3 AND phase = 'delete_started') OR (phase_index = 4 AND phase = 'remote_absent')),
    evidence_sha256 TEXT NOT NULL CHECK (length(evidence_sha256) = 64 AND evidence_sha256 NOT GLOB '*[^0-9a-f]*'),
    runner_id INTEGER CHECK (runner_id IS NULL OR runner_id > 0),
    runner_name TEXT CHECK (runner_name IS NULL OR length(CAST(runner_name AS BLOB)) BETWEEN 1 AND 128),
    runner_scale_set_id INTEGER CHECK (runner_scale_set_id IS NULL OR runner_scale_set_id > 0),
    request_sha256 TEXT CHECK (request_sha256 IS NULL OR (length(request_sha256) = 64 AND request_sha256 NOT GLOB '*[^0-9a-f]*')),
    response_sha256 TEXT CHECK (response_sha256 IS NULL OR (length(response_sha256) = 64 AND response_sha256 NOT GLOB '*[^0-9a-f]*')),
    remove_status INTEGER CHECK (remove_status IS NULL OR remove_status = 204),
    lookup_status INTEGER CHECK (lookup_status IS NULL OR lookup_status IN (200, 404)),
    lookup_count INTEGER CHECK (lookup_count IS NULL OR lookup_count >= 0),
    observed_at_unix_ms INTEGER NOT NULL CHECK (observed_at_unix_ms > 0),
    PRIMARY KEY (reservation_id, claim_generation, phase_index),
    FOREIGN KEY (reservation_id, claim_generation) REFERENCES physical_attempt_jit_conflict_recovery_claims(reservation_id, claim_generation),
    CHECK ((phase_index = 1 AND runner_id IS NULL AND runner_name IS NULL AND runner_scale_set_id IS NULL AND request_sha256 IS NULL AND response_sha256 IS NULL AND remove_status IS NULL AND lookup_status IS NULL AND lookup_count IS NULL) OR (phase_index = 2 AND phase = 'remote_target' AND runner_id IS NOT NULL AND runner_name IS NOT NULL AND runner_scale_set_id IS NOT NULL AND request_sha256 IS NOT NULL AND response_sha256 IS NOT NULL AND remove_status IS NULL AND lookup_status = 200 AND lookup_count = 1) OR (phase_index = 2 AND phase = 'remote_initial_absent' AND runner_id IS NULL AND runner_name IS NOT NULL AND runner_scale_set_id IS NOT NULL AND request_sha256 IS NOT NULL AND response_sha256 IS NOT NULL AND remove_status IS NULL AND lookup_status = 200 AND lookup_count = 0) OR (phase_index = 3 AND runner_id IS NOT NULL AND runner_name IS NOT NULL AND runner_scale_set_id IS NOT NULL AND request_sha256 IS NULL AND response_sha256 IS NULL AND remove_status IS NULL AND lookup_status IS NULL AND lookup_count IS NULL) OR (phase_index = 4 AND runner_name IS NOT NULL AND runner_scale_set_id IS NOT NULL AND request_sha256 IS NOT NULL AND response_sha256 IS NOT NULL AND remove_status IS NULL AND lookup_status = 200 AND lookup_count = 0)),
    CHECK (phase_index NOT IN (2,4) OR (request_sha256 IS NOT NULL AND response_sha256 IS NOT NULL AND lookup_status IS NOT NULL AND lookup_count IS NOT NULL))
)";

pub(super) const SETTLEMENTS: &str = r"CREATE TABLE physical_attempt_jit_conflict_settlements (
    lineage_id INTEGER NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    reservation_id INTEGER NOT NULL UNIQUE,
    claim_generation INTEGER NOT NULL CHECK (claim_generation > 0),
    metadata_generation INTEGER NOT NULL CHECK (metadata_generation > 0),
    remote_runner_id INTEGER CHECK (remote_runner_id IS NULL OR remote_runner_id > 0),
    remote_runner_name TEXT NOT NULL CHECK (length(CAST(remote_runner_name AS BLOB)) BETWEEN 1 AND 128),
    remote_scale_set_id INTEGER NOT NULL CHECK (remote_scale_set_id > 0),
    proof_digest TEXT NOT NULL CHECK (length(proof_digest) = 64 AND proof_digest NOT GLOB '*[^0-9a-f]*'),
    settlement_digest TEXT NOT NULL CHECK (length(settlement_digest) = 64 AND settlement_digest NOT GLOB '*[^0-9a-f]*'),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    PRIMARY KEY (lineage_id, generation),
    UNIQUE (lineage_id, generation, reservation_id),
    CHECK (proof_digest = settlement_digest),
    FOREIGN KEY (lineage_id, generation, reservation_id) REFERENCES physical_attempts(lineage_id, generation, reservation_id),
    FOREIGN KEY (reservation_id, claim_generation) REFERENCES physical_attempt_jit_conflict_recovery_claims(reservation_id, claim_generation),
    FOREIGN KEY (reservation_id, metadata_generation) REFERENCES capacity_reservation_metadata_settlements(reservation_id, generation)
)";

pub(super) const TERMINAL_PROOFS_VIEW: &str = r"CREATE VIEW physical_attempt_terminal_proofs AS
SELECT lineage_id, generation, reservation_id, outcome, proof_digest, settlement_digest, created_at
FROM physical_attempt_outcomes
UNION ALL
SELECT lineage_id, generation, reservation_id, 'post_jit_retired' AS outcome, proof_digest, settlement_digest, created_at
FROM physical_attempt_post_jit_retirements
UNION ALL
SELECT lineage_id, generation, reservation_id, 'jit_conflict_retired' AS outcome, proof_digest, settlement_digest, created_at
FROM physical_attempt_jit_conflict_settlements";

pub(super) const JIT_INDEXES: [(&str, &str); 5] = [
    (
        "connection_runner_jit_id",
        "CREATE UNIQUE INDEX connection_runner_jit_id ON connection_runner_jits(connection_id, scale_set_id, runner_id) WHERE runner_id IS NOT NULL",
    ),
    (
        "connection_runner_jit_name",
        "CREATE UNIQUE INDEX connection_runner_jit_name ON connection_runner_jits(connection_id, scale_set_id, runner_name) WHERE runner_name IS NOT NULL AND state != 'cleaned'",
    ),
    (
        "connection_runner_jit_witness",
        "CREATE UNIQUE INDEX connection_runner_jit_witness ON connection_runner_jits(connection_id, scale_set_id, witness_request_id) WHERE state NOT IN ('cleaned', 'conflict_retired')",
    ),
    (
        "connection_runner_jit_started_request",
        "CREATE UNIQUE INDEX connection_runner_jit_started_request ON connection_runner_jits(connection_id, scale_set_id, started_request_id) WHERE started_request_id IS NOT NULL",
    ),
    (
        "connection_runner_jit_completed_request",
        "CREATE UNIQUE INDEX connection_runner_jit_completed_request ON connection_runner_jits(connection_id, scale_set_id, completed_request_id) WHERE completed_request_id IS NOT NULL",
    ),
];

pub(super) const TABLE_NAMES: [&str; 3] = [
    "physical_attempt_jit_conflict_recovery_claims",
    "physical_attempt_jit_conflict_recovery_progress",
    "physical_attempt_jit_conflict_settlements",
];
