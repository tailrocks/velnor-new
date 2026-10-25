//! Database definitions for physical retirement records.

pub(super) const TABLE_NAMES: [&str; 4] = [
    "physical_attempt_post_jit_retirement_claims",
    "physical_attempt_post_jit_retirement_progress",
    "physical_attempt_post_jit_retirement_remote_absence",
    "physical_attempt_post_jit_retirements",
];

pub(super) const CLAIMS_DDL: &str = r"CREATE TABLE IF NOT EXISTS physical_attempt_post_jit_retirement_claims (
    reservation_id INTEGER NOT NULL,
    claim_generation INTEGER NOT NULL CHECK (claim_generation > 0),
    lineage_id INTEGER NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    intent_id INTEGER NOT NULL,
    connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id),
    scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0),
    witness_request_id INTEGER NOT NULL CHECK (witness_request_id > 0),
    binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'),
    source_digest TEXT NOT NULL CHECK (length(source_digest) = 64 AND source_digest NOT GLOB '*[^0-9a-f]*'),
    acquire_response_digest TEXT NOT NULL CHECK (length(acquire_response_digest) = 64 AND acquire_response_digest NOT GLOB '*[^0-9a-f]*'),
    engine_id TEXT NOT NULL CHECK (length(CAST(engine_id AS BLOB)) BETWEEN 1 AND 256),
    docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'),
    instance_binding_digest TEXT NOT NULL CHECK (length(instance_binding_digest) = 64 AND instance_binding_digest NOT GLOB '*[^0-9a-f]*'),
    runner_id INTEGER NOT NULL CHECK (runner_id > 0),
    runner_name TEXT NOT NULL CHECK (length(CAST(runner_name AS BLOB)) BETWEEN 1 AND 128),
    snapshot_sha256 TEXT NOT NULL CHECK (length(snapshot_sha256) = 64 AND snapshot_sha256 NOT GLOB '*[^0-9a-f]*'),
    lease_until INTEGER NOT NULL CHECK (lease_until >= 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    PRIMARY KEY (reservation_id, claim_generation),
    FOREIGN KEY (lineage_id, generation, reservation_id) REFERENCES physical_attempts(lineage_id, generation, reservation_id),
    FOREIGN KEY (lineage_id, generation, intent_id) REFERENCES physical_attempts(lineage_id, generation, intent_id)
)";

pub(super) const PROGRESS_DDL: &str = r"CREATE TABLE IF NOT EXISTS physical_attempt_post_jit_retirement_progress (
    reservation_id INTEGER NOT NULL,
    claim_generation INTEGER NOT NULL CHECK (claim_generation > 0),
    phase_index INTEGER NOT NULL CHECK (phase_index BETWEEN 0 AND 5),
    phase TEXT NOT NULL CHECK (
        (phase_index = 0 AND phase = 'claimed') OR
        (phase_index = 1 AND phase = 'runner_stopped') OR
        (phase_index = 2 AND phase = 'remote_absent') OR
        (phase_index = 3 AND phase = 'storage_absent') OR
        (phase_index = 4 AND phase = 'network_absent') OR
        (phase_index = 5 AND phase = 'capacity_settled')
    ),
    evidence_sha256 TEXT NOT NULL CHECK (length(evidence_sha256) = 64 AND evidence_sha256 NOT GLOB '*[^0-9a-f]*'),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    PRIMARY KEY (reservation_id, claim_generation, phase_index),
    FOREIGN KEY (reservation_id, claim_generation) REFERENCES physical_attempt_post_jit_retirement_claims(reservation_id, claim_generation)
)";

pub(super) const REMOTE_ABSENCE_DDL: &str = r"CREATE TABLE IF NOT EXISTS physical_attempt_post_jit_retirement_remote_absence (
    reservation_id INTEGER NOT NULL,
    claim_generation INTEGER NOT NULL CHECK (claim_generation > 0),
    runner_id INTEGER NOT NULL CHECK (runner_id > 0),
    runner_name TEXT NOT NULL CHECK (length(CAST(runner_name AS BLOB)) BETWEEN 1 AND 128),
    scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0),
    lookup_status INTEGER NOT NULL CHECK (lookup_status = 200),
    lookup_count INTEGER NOT NULL CHECK (lookup_count = 0),
    lookup_value_count INTEGER NOT NULL CHECK (lookup_value_count = 0),
    request_sha256 TEXT NOT NULL CHECK (length(request_sha256) = 64 AND request_sha256 NOT GLOB '*[^0-9a-f]*'),
    response_sha256 TEXT NOT NULL CHECK (length(response_sha256) = 64 AND response_sha256 NOT GLOB '*[^0-9a-f]*'),
    remove_status INTEGER CHECK (remove_status IS NULL OR remove_status = 204),
    observed_at_unix_ms INTEGER NOT NULL CHECK (observed_at_unix_ms > 0),
    PRIMARY KEY (reservation_id, claim_generation),
    FOREIGN KEY (reservation_id, claim_generation) REFERENCES physical_attempt_post_jit_retirement_claims(reservation_id, claim_generation)
)";

pub(super) const RETIREMENTS_DDL: &str = r"CREATE TABLE IF NOT EXISTS physical_attempt_post_jit_retirements (
    lineage_id INTEGER NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    reservation_id INTEGER NOT NULL,
    claim_generation INTEGER NOT NULL CHECK (claim_generation > 0),
    metadata_generation INTEGER NOT NULL CHECK (metadata_generation > 0),
    cleanup_probe_operation_id TEXT NOT NULL REFERENCES resource_probe_operations(operation_id),
    proof_digest TEXT NOT NULL CHECK (length(proof_digest) = 64 AND proof_digest NOT GLOB '*[^0-9a-f]*'),
    settlement_digest TEXT NOT NULL CHECK (length(settlement_digest) = 64 AND settlement_digest NOT GLOB '*[^0-9a-f]*'),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    PRIMARY KEY (lineage_id, generation),
    UNIQUE (reservation_id),
    UNIQUE (lineage_id, generation, reservation_id),
    CHECK (proof_digest = settlement_digest),
    FOREIGN KEY (lineage_id, generation, reservation_id) REFERENCES physical_attempts(lineage_id, generation, reservation_id),
    FOREIGN KEY (reservation_id, claim_generation) REFERENCES physical_attempt_post_jit_retirement_claims(reservation_id, claim_generation),
    FOREIGN KEY (reservation_id, claim_generation) REFERENCES physical_attempt_post_jit_retirement_remote_absence(reservation_id, claim_generation),
    FOREIGN KEY (reservation_id, metadata_generation) REFERENCES capacity_reservation_metadata_settlements(reservation_id, generation)
)";

pub(super) const TERMINAL_PROOFS_VIEW: &str = r"CREATE VIEW physical_attempt_terminal_proofs AS
SELECT lineage_id, generation, reservation_id, outcome, proof_digest, settlement_digest, created_at
FROM physical_attempt_outcomes
UNION ALL
SELECT lineage_id, generation, reservation_id, 'post_jit_retired' AS outcome, proof_digest, settlement_digest, created_at
FROM physical_attempt_post_jit_retirements";
