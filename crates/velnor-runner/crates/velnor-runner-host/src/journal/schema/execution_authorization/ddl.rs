//! Exact execution-authority schema objects.

use super::super::sql_normalization::normalize;
mod guards;
mod legacy_v27;
mod upgrade_v28;

#[cfg(test)]
#[path = "../execution_authorization_ddl_tests.rs"]
mod tests;

use crate::error::HostError;

const POLICY_TABLE: &str = r"CREATE TABLE IF NOT EXISTS connection_job_authority_policies (
    connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id),
    generation INTEGER NOT NULL CHECK (generation > 0),
    binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'),
    policy_digest TEXT NOT NULL CHECK (length(policy_digest) = 64 AND policy_digest NOT GLOB '*[^0-9a-f]*'),
    policy_json TEXT NOT NULL CHECK (length(CAST(policy_json AS BLOB)) BETWEEN 2 AND 262144),
    PRIMARY KEY (connection_id, generation),
    UNIQUE (connection_id, generation, policy_digest)
)";

const ACTIVE_TABLE: &str = r"CREATE TABLE IF NOT EXISTS connection_active_job_authority (
    connection_id TEXT NOT NULL PRIMARY KEY REFERENCES connection_scopes(connection_id),
    generation INTEGER NOT NULL CHECK (generation > 0),
    policy_digest TEXT NOT NULL CHECK (length(policy_digest) = 64 AND policy_digest NOT GLOB '*[^0-9a-f]*'),
    FOREIGN KEY (connection_id, generation, policy_digest)
        REFERENCES connection_job_authority_policies(connection_id, generation, policy_digest)
)";

const PROOF_TABLE: &str = r"CREATE TABLE IF NOT EXISTS connection_execution_proofs (
    intent_id INTEGER NOT NULL PRIMARY KEY REFERENCES connection_intents(id),
    reservation_id INTEGER NOT NULL UNIQUE REFERENCES capacity_reservations(id),
    connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id),
    scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0),
    witness_request_id INTEGER NOT NULL CHECK (witness_request_id > 0),
    binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'),
    generation INTEGER NOT NULL CHECK (generation > 0),
    policy_digest TEXT NOT NULL CHECK (length(policy_digest) = 64 AND policy_digest NOT GLOB '*[^0-9a-f]*'),
    engine_id TEXT NOT NULL CHECK (length(CAST(engine_id AS BLOB)) BETWEEN 1 AND 128),
    docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'),
    runner_id INTEGER NOT NULL CHECK (runner_id > 0),
    runner_name TEXT NOT NULL CHECK (length(CAST(runner_name AS BLOB)) BETWEEN 1 AND 128),
    runner_container_id TEXT NOT NULL CHECK (length(CAST(runner_container_id AS BLOB)) BETWEEN 12 AND 128),
    dind_container_id TEXT NOT NULL CHECK (length(CAST(dind_container_id AS BLOB)) BETWEEN 12 AND 128),
    instance_binding_digest TEXT NOT NULL CHECK (length(instance_binding_digest) = 64 AND instance_binding_digest NOT GLOB '*[^0-9a-f]*'),
    challenge TEXT NOT NULL UNIQUE CHECK (length(challenge) = 64 AND challenge NOT GLOB '*[^0-9a-f]*'),
    channel_digest TEXT NOT NULL CHECK (length(channel_digest) = 64 AND channel_digest NOT GLOB '*[^0-9a-f]*'),
    channel_json TEXT NOT NULL CHECK (length(CAST(channel_json AS BLOB)) BETWEEN 2 AND 16384),
    challenge_state TEXT NOT NULL CHECK (challenge_state IN ('not_requested', 'requested', 'published', 'uncertain', 'abandoned')),
    challenge_json TEXT NOT NULL CHECK (length(CAST(challenge_json AS BLOB)) BETWEEN 2 AND 8192),
    challenge_digest TEXT NOT NULL CHECK (length(challenge_digest) = 64 AND challenge_digest NOT GLOB '*[^0-9a-f]*'),
    challenge_file_path TEXT CHECK (challenge_file_path IS NULL OR challenge_file_path = '/opt/velnor/source-proof/control/challenge.json'),
    challenge_file_inode INTEGER CHECK (challenge_file_inode IS NULL OR challenge_file_inode > 0),
    challenge_file_bytes INTEGER CHECK (challenge_file_bytes IS NULL OR challenge_file_bytes BETWEEN 1 AND 8192),
    challenge_file_uid INTEGER CHECK (challenge_file_uid IS NULL OR challenge_file_uid = 0),
    challenge_file_gid INTEGER CHECK (challenge_file_gid IS NULL OR challenge_file_gid = 0),
    challenge_file_mode INTEGER CHECK (challenge_file_mode IS NULL OR challenge_file_mode = 292),
    challenge_mount_id INTEGER CHECK (challenge_mount_id IS NULL OR challenge_mount_id > 0),
    challenge_device_major INTEGER CHECK (challenge_device_major IS NULL OR challenge_device_major >= 0),
    challenge_device_minor INTEGER CHECK (challenge_device_minor IS NULL OR challenge_device_minor >= 0),
    challenge_statx_device_major INTEGER CHECK (challenge_statx_device_major IS NULL OR challenge_statx_device_major >= 0),
    challenge_statx_device_minor INTEGER CHECK (challenge_statx_device_minor IS NULL OR challenge_statx_device_minor >= 0),
    challenge_fs_type_magic TEXT CHECK (challenge_fs_type_magic IS NULL OR (length(challenge_fs_type_magic) = 16 AND challenge_fs_type_magic NOT GLOB '*[^0-9a-f]*')),
    challenge_filesystem_id TEXT CHECK (challenge_filesystem_id IS NULL OR (length(challenge_filesystem_id) = 16 AND challenge_filesystem_id NOT GLOB '*[^0-9a-f]*')),
    challenge_filesystem_instance_digest TEXT CHECK (challenge_filesystem_instance_digest IS NULL OR (length(challenge_filesystem_instance_digest) = 64 AND challenge_filesystem_instance_digest NOT GLOB '*[^0-9a-f]*')),
    challenge_read_only INTEGER CHECK (challenge_read_only IS NULL OR challenge_read_only = 1),
    challenge_no_copy INTEGER CHECK (challenge_no_copy IS NULL OR challenge_no_copy = 1),
    source_digest TEXT CHECK (source_digest IS NULL OR (length(source_digest) = 64 AND source_digest NOT GLOB '*[^0-9a-f]*')),
    source_json TEXT CHECK (source_json IS NULL OR length(CAST(source_json AS BLOB)) BETWEEN 2 AND 131072),
    proof_state TEXT NOT NULL CHECK (proof_state IN ('pending', 'approved', 'denied')),
    actual_request_id INTEGER CHECK (actual_request_id IS NULL OR actual_request_id > 0),
    started_event_digest TEXT CHECK (started_event_digest IS NULL OR (length(started_event_digest) = 64 AND started_event_digest NOT GLOB '*[^0-9a-f]*')),
    run_id INTEGER CHECK (run_id IS NULL OR run_id > 0),
    run_attempt INTEGER CHECK (run_attempt IS NULL OR run_attempt > 0),
    workflow_job_key TEXT CHECK (workflow_job_key IS NULL OR length(CAST(workflow_job_key AS BLOB)) BETWEEN 1 AND 128),
    authority_role TEXT CHECK (authority_role IS NULL OR authority_role IN ('verification', 'trusted_publication')),
    native_job_id TEXT CHECK (native_job_id IS NULL OR length(CAST(native_job_id AS BLOB)) BETWEEN 1 AND 256),
    rest_job_id TEXT CHECK (rest_job_id IS NULL OR length(CAST(rest_job_id AS BLOB)) BETWEEN 1 AND 128),
    deny_code TEXT CHECK (deny_code IS NULL OR deny_code IN ('source_mismatch', 'workflow_not_approved', 'policy_changed', 'proof_malformed', 'interrupted_after_restart', 'association_mismatch', 'runner_terminated')),
    FOREIGN KEY (connection_id, generation, policy_digest)
        REFERENCES connection_job_authority_policies(connection_id, generation, policy_digest),
    FOREIGN KEY (connection_id, scale_set_id, witness_request_id)
        REFERENCES connection_request_authorizations(connection_id, scale_set_id, request_id),
    FOREIGN KEY (connection_id, scale_set_id, actual_request_id)
        REFERENCES connection_request_authorizations(connection_id, scale_set_id, request_id),
    CHECK ((source_digest IS NULL AND source_json IS NULL) OR (source_digest IS NOT NULL AND source_json IS NOT NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_file_bytes IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_file_uid IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_file_gid IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_file_mode IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_mount_id IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_device_major IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_device_minor IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_statx_device_major IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_statx_device_minor IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_fs_type_magic IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_filesystem_id IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_filesystem_instance_digest IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_read_only IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_no_copy IS NULL)),
    CHECK ((challenge_file_inode IS NULL) = (challenge_file_path IS NULL)),
    CHECK (challenge_state != 'published' OR challenge_file_inode IS NOT NULL),
    CHECK ((run_id IS NULL AND run_attempt IS NULL) OR (run_id IS NOT NULL AND run_attempt IS NOT NULL)),
    CHECK (proof_state != 'pending' OR actual_request_id IS NULL),
    CHECK (proof_state != 'pending' OR started_event_digest IS NULL),
    CHECK (proof_state != 'pending' OR run_id IS NULL),
    CHECK (proof_state != 'pending' OR run_attempt IS NULL),
    CHECK (proof_state != 'pending' OR workflow_job_key IS NULL),
    CHECK (proof_state != 'pending' OR authority_role IS NULL),
    CHECK (proof_state != 'pending' OR native_job_id IS NULL),
    CHECK (proof_state != 'pending' OR rest_job_id IS NULL),
    CHECK (proof_state != 'pending' OR deny_code IS NULL),
    CHECK (proof_state != 'approved' OR source_digest IS NOT NULL),
    CHECK (proof_state != 'approved' OR actual_request_id IS NOT NULL),
    CHECK (proof_state != 'approved' OR started_event_digest IS NOT NULL),
    CHECK (proof_state != 'approved' OR run_id IS NOT NULL),
    CHECK (proof_state != 'approved' OR run_attempt IS NOT NULL),
    CHECK (proof_state != 'approved' OR workflow_job_key IS NOT NULL),
    CHECK (proof_state != 'approved' OR authority_role IS NOT NULL),
    CHECK (proof_state != 'approved' OR native_job_id IS NOT NULL),
    CHECK (proof_state != 'approved' OR deny_code IS NULL),
    CHECK (proof_state != 'denied' OR deny_code IS NOT NULL)
)";

const DECISION_TABLE: &str = r"CREATE TABLE IF NOT EXISTS connection_execution_decisions (
    intent_id INTEGER NOT NULL PRIMARY KEY REFERENCES connection_execution_proofs(intent_id),
    reservation_id INTEGER NOT NULL UNIQUE REFERENCES capacity_reservations(id),
    connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id),
    scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0),
    witness_request_id INTEGER NOT NULL CHECK (witness_request_id > 0),
    actual_request_id INTEGER NOT NULL CHECK (actual_request_id > 0),
    challenge TEXT NOT NULL CHECK (length(challenge) = 64 AND challenge NOT GLOB '*[^0-9a-f]*'),
    generation INTEGER NOT NULL CHECK (generation > 0),
    policy_digest TEXT NOT NULL CHECK (length(policy_digest) = 64 AND policy_digest NOT GLOB '*[^0-9a-f]*'),
    request_digest TEXT NOT NULL CHECK (length(request_digest) = 64 AND request_digest NOT GLOB '*[^0-9a-f]*'),
    allow INTEGER NOT NULL CHECK (allow IN (0, 1)),
    decision_digest TEXT NOT NULL CHECK (length(decision_digest) = 64 AND decision_digest NOT GLOB '*[^0-9a-f]*'),
    decision_json TEXT NOT NULL CHECK (length(CAST(decision_json AS BLOB)) BETWEEN 2 AND 8192),
    delivery_state TEXT NOT NULL CHECK (delivery_state IN ('prepared', 'requested', 'delivered', 'uncertain', 'abandoned')),
    decision_file_path TEXT CHECK (decision_file_path IS NULL OR decision_file_path = '/opt/velnor/source-proof/control/decision.json'),
    decision_file_inode INTEGER CHECK (decision_file_inode IS NULL OR decision_file_inode > 0),
    decision_file_bytes INTEGER CHECK (decision_file_bytes IS NULL OR decision_file_bytes BETWEEN 1 AND 8192),
    decision_file_uid INTEGER CHECK (decision_file_uid IS NULL OR decision_file_uid = 0),
    decision_file_gid INTEGER CHECK (decision_file_gid IS NULL OR decision_file_gid = 0),
    decision_file_mode INTEGER CHECK (decision_file_mode IS NULL OR decision_file_mode = 292),
    decision_mount_id INTEGER CHECK (decision_mount_id IS NULL OR decision_mount_id > 0),
    decision_device_major INTEGER CHECK (decision_device_major IS NULL OR decision_device_major >= 0),
    decision_device_minor INTEGER CHECK (decision_device_minor IS NULL OR decision_device_minor >= 0),
    decision_statx_device_major INTEGER CHECK (decision_statx_device_major IS NULL OR decision_statx_device_major >= 0),
    decision_statx_device_minor INTEGER CHECK (decision_statx_device_minor IS NULL OR decision_statx_device_minor >= 0),
    decision_fs_type_magic TEXT CHECK (decision_fs_type_magic IS NULL OR (length(decision_fs_type_magic) = 16 AND decision_fs_type_magic NOT GLOB '*[^0-9a-f]*')),
    decision_filesystem_id TEXT CHECK (decision_filesystem_id IS NULL OR (length(decision_filesystem_id) = 16 AND decision_filesystem_id NOT GLOB '*[^0-9a-f]*')),
    decision_filesystem_instance_digest TEXT CHECK (decision_filesystem_instance_digest IS NULL OR (length(decision_filesystem_instance_digest) = 64 AND decision_filesystem_instance_digest NOT GLOB '*[^0-9a-f]*')),
    decision_read_only INTEGER CHECK (decision_read_only IS NULL OR decision_read_only = 1),
    decision_no_copy INTEGER CHECK (decision_no_copy IS NULL OR decision_no_copy = 1),
    FOREIGN KEY (connection_id, scale_set_id, actual_request_id)
        REFERENCES connection_request_authorizations(connection_id, scale_set_id, request_id),
    CHECK ((decision_file_inode IS NULL) = (decision_file_bytes IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_file_uid IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_file_gid IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_file_mode IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_mount_id IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_device_major IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_device_minor IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_statx_device_major IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_statx_device_minor IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_fs_type_magic IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_filesystem_id IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_filesystem_instance_digest IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_read_only IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_no_copy IS NULL)),
    CHECK ((decision_file_inode IS NULL) = (decision_file_path IS NULL)),
    CHECK (delivery_state != 'delivered' OR decision_file_inode IS NOT NULL)
)";

const TABLES: &[&str] = &[POLICY_TABLE, ACTIVE_TABLE, PROOF_TABLE, DECISION_TABLE];

const TABLE_OBJECTS: &[(&str, &str, &str)] = &[
    ("table", "connection_job_authority_policies", POLICY_TABLE),
    ("table", "connection_active_job_authority", ACTIVE_TABLE),
    ("table", "connection_execution_proofs", PROOF_TABLE),
    ("table", "connection_execution_decisions", DECISION_TABLE),
];

pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
    let proof_table_existed =
        object_exists(connection, "table", "connection_execution_proofs").await?;
    let decision_table_existed =
        object_exists(connection, "table", "connection_execution_decisions").await?;
    for statement in TABLES {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    guards::bootstrap_decision_transition_guards(connection, decision_table_existed).await?;
    for (_, _, statement) in guards::OBJECTS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    guards::bootstrap_proof_insert_guards(connection, proof_table_existed).await?;
    guards::bootstrap_proof_transition_guards(connection, proof_table_existed).await?;
    install_revision_triggers(connection).await
}

async fn object_exists(
    connection: &turso::Connection,
    kind: &str,
    name: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = ?1 AND name = ?2)",
            (kind, name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map(|value| value != 0)
        .map_err(|_| HostError::Journal)
}

pub(super) async fn validate_objects(connection: &turso::Connection) -> Result<(), HostError> {
    validate_objects_with_tables(
        connection,
        PROOF_TABLE,
        DECISION_TABLE,
        guards::PROOF_TRANSITION_GUARDS,
    )
    .await
}

pub(super) async fn validate_v27_objects(connection: &turso::Connection) -> Result<(), HostError> {
    validate_objects_with_tables(
        connection,
        legacy_v27::PROOF_TABLE,
        legacy_v27::DECISION_TABLE,
        guards::PROOF_TRANSITION_GUARDS,
    )
    .await
}

async fn validate_objects_with_tables(
    connection: &turso::Connection,
    proof_table: &str,
    decision_table: &str,
    proof_transition_guards: &[(&str, &str)],
) -> Result<(), HostError> {
    for (kind, name, expected) in TABLE_OBJECTS.iter().chain(guards::OBJECTS.iter()) {
        let expected = match *name {
            "connection_execution_proofs" => proof_table,
            "connection_execution_decisions" => decision_table,
            _ => expected,
        };
        let actual = object_sql(connection, kind, name).await?;
        if normalize(&actual) != normalize(expected) {
            return Err(HostError::Journal);
        }
    }
    for (name, expected) in guards::PROOF_INSERT_GUARDS {
        let actual = object_sql(connection, "trigger", name).await?;
        if normalize(&actual) != normalize(expected) {
            return Err(HostError::Journal);
        }
    }
    for (name, expected) in proof_transition_guards {
        let actual = object_sql(connection, "trigger", name).await?;
        if normalize(&actual) != normalize(expected) {
            return Err(HostError::Journal);
        }
    }
    for (name, expected) in guards::DECISION_TRANSITION_GUARDS {
        let actual = object_sql(connection, "trigger", name).await?;
        if normalize(&actual) != normalize(expected) {
            return Err(HostError::Journal);
        }
    }
    validate_revision_triggers(connection).await
}

pub(super) async fn upgrade_v28(connection: &turso::Connection) -> Result<(), HostError> {
    upgrade_v28::upgrade(connection).await
}

async fn install_revision_triggers(connection: &turso::Connection) -> Result<(), HostError> {
    for (table, prefix) in [
        (
            "connection_job_authority_policies",
            "job_authority_policies",
        ),
        ("connection_active_job_authority", "active_job_authority"),
        ("connection_execution_proofs", "execution_proofs"),
        ("connection_execution_decisions", "execution_decisions"),
    ] {
        for action in ["insert", "update", "delete"] {
            let name = format!("execution_{prefix}_revision_{action}");
            let sql = format!(
                "CREATE TRIGGER IF NOT EXISTS {name} AFTER {action} ON {table} BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END"
            );
            connection
                .execute(&sql, ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    Ok(())
}

async fn validate_revision_triggers(connection: &turso::Connection) -> Result<(), HostError> {
    for (table, prefix) in [
        (
            "connection_job_authority_policies",
            "job_authority_policies",
        ),
        ("connection_active_job_authority", "active_job_authority"),
        ("connection_execution_proofs", "execution_proofs"),
        ("connection_execution_decisions", "execution_decisions"),
    ] {
        for action in ["insert", "update", "delete"] {
            let name = format!("execution_{prefix}_revision_{action}");
            let expected = format!(
                "CREATE TRIGGER {name} AFTER {action} ON {table} BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END"
            );
            if normalize(&object_sql(connection, "trigger", &name).await?) != normalize(&expected) {
                return Err(HostError::Journal);
            }
        }
    }
    Ok(())
}

async fn object_sql(
    connection: &turso::Connection,
    kind: &str,
    name: &str,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
            (kind, name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}
