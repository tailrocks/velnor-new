// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use turso::Connection;

use crate::journal::Journal;

pub(super) type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

pub(super) const PROOF_TABLE: &str = "connection_execution_proofs";
pub(super) const GUARDS: [&str; 4] = [
    "execution_proof_source_immutable_guard",
    "execution_proof_published_file_guard",
    "execution_proof_published_mount_guard",
    "execution_proof_published_filesystem_guard",
];

pub(super) const DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000001";
pub(super) const OTHER_DIGEST: &str =
    "0000000000000000000000000000000000000000000000000000000000000002";
pub(super) const PUBLISHED_CHALLENGE: &str =
    "0000000000000000000000000000000000000000000000000000000000000004";
pub(super) const CHALLENGE_JSON: &str = r#"{"challenge":1}"#;
pub(super) const SOURCE_JSON: &str = r#"{"source":1}"#;
pub(super) const OTHER_SOURCE_JSON: &str = r#"{"source":2}"#;
pub(super) const FILE_COLUMNS: [&str; 16] = [
    "challenge_file_path",
    "challenge_file_inode",
    "challenge_file_bytes",
    "challenge_file_uid",
    "challenge_file_gid",
    "challenge_file_mode",
    "challenge_mount_id",
    "challenge_device_major",
    "challenge_device_minor",
    "challenge_statx_device_major",
    "challenge_statx_device_minor",
    "challenge_fs_type_magic",
    "challenge_filesystem_id",
    "challenge_filesystem_instance_digest",
    "challenge_read_only",
    "challenge_no_copy",
];

const PARENT_TABLES: [&str; 5] = [
    "CREATE TABLE connection_intents (id INTEGER PRIMARY KEY)",
    "CREATE TABLE capacity_reservations (id INTEGER PRIMARY KEY)",
    "CREATE TABLE connection_scopes (connection_id TEXT PRIMARY KEY)",
    "CREATE TABLE connection_request_authorizations (connection_id TEXT NOT NULL, scale_set_id INTEGER NOT NULL, request_id INTEGER NOT NULL, PRIMARY KEY (connection_id, scale_set_id, request_id))",
    "CREATE TABLE connection_job_authority_policies (connection_id TEXT NOT NULL, generation INTEGER NOT NULL, policy_digest TEXT NOT NULL, binding_digest TEXT NOT NULL, PRIMARY KEY (connection_id, generation, policy_digest))",
];

const PROOF_COLUMNS: &str = "intent_id, reservation_id, connection_id, scale_set_id, witness_request_id, binding_digest, generation, policy_digest, engine_id, docker_root_digest, runner_id, runner_name, runner_container_id, dind_container_id, instance_binding_digest, challenge, channel_digest, channel_json, challenge_state, challenge_json, challenge_digest, proof_state";

pub(super) async fn object_sql(
    connection: &Connection,
    kind: &str,
    name: &str,
) -> TestResult<String> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
            (kind, name),
        )
        .await?;
    let row = rows.next().await?.ok_or("missing object")?;
    Ok(row.get::<String>(0)?)
}

async fn scratch_connection(dir: &tempfile::TempDir, name: &str) -> TestResult<Connection> {
    let path = dir.path().join(format!("{name}.db"));
    let text = path.to_str().ok_or("scratch path")?;
    let database = turso::Builder::new_local(text).build().await?;
    Ok(database.connect()?)
}

pub(super) async fn journal_proof_guards(journal: &Journal) -> TestResult<(String, Vec<String>)> {
    let source = journal.connection().await?;
    let proof_table = object_sql(&source, "table", PROOF_TABLE).await?;
    let mut guards = Vec::with_capacity(GUARDS.len());
    for name in GUARDS {
        guards.push(object_sql(&source, "trigger", name).await?);
    }
    Ok((proof_table, guards))
}

pub(super) async fn scratch_database(
    dir: &tempfile::TempDir,
    name: &str,
    proof_table: &str,
    guards: &[String],
) -> TestResult<Connection> {
    let connection = scratch_connection(dir, name).await?;
    for statement in PARENT_TABLES {
        connection.execute(statement, ()).await?;
    }
    connection
        .execute(
            "INSERT INTO connection_scopes (connection_id) VALUES ('connection')",
            (),
        )
        .await?;
    connection
        .execute(
            "INSERT INTO connection_job_authority_policies (connection_id, generation, policy_digest, binding_digest) VALUES ('connection', 1, ?1, ?1)",
            (DIGEST,),
        )
        .await?;
    connection.execute(proof_table, ()).await?;
    for guard in guards {
        connection.execute(guard, ()).await?;
    }
    Ok(connection)
}

pub(super) async fn proof_guards_database(
    dir: &tempfile::TempDir,
    name: &str,
    journal: &Journal,
) -> TestResult<Connection> {
    let (proof_table, guards) = journal_proof_guards(journal).await?;
    scratch_database(dir, name, &proof_table, &guards).await
}

async fn insert_parents(connection: &Connection, id: i64) -> TestResult<()> {
    connection
        .execute("INSERT INTO connection_intents (id) VALUES (?1)", (id,))
        .await?;
    connection
        .execute("INSERT INTO capacity_reservations (id) VALUES (?1)", (id,))
        .await?;
    connection
        .execute(
            "INSERT INTO connection_request_authorizations (connection_id, scale_set_id, request_id) VALUES ('connection', 1, ?1)",
            (id,),
        )
        .await?;
    Ok(())
}

pub(super) async fn insert_proof(
    connection: &Connection,
    id: i64,
    challenge: &str,
    challenge_state: &str,
) -> TestResult<()> {
    insert_parents(connection, id).await?;
    connection
        .execute(
            &format!(
                "INSERT INTO {PROOF_TABLE} ({PROOF_COLUMNS}) VALUES (?1, ?1, 'connection', 1, ?1, ?2, 1, ?2, 'engine', ?2, 7, 'runner', 'runner-container', 'dind-container', ?2, ?3, ?2, '{{\"channel\":1}}', ?4, ?5, ?2, 'pending')"
            ),
            (id, DIGEST, challenge, challenge_state, CHALLENGE_JSON),
        )
        .await?;
    Ok(())
}

pub(super) async fn record_source(
    connection: &Connection,
    id: i64,
    digest: &str,
    json: &str,
) -> TestResult<()> {
    connection
        .execute(
            &format!(
                "UPDATE {PROOF_TABLE} SET source_digest = ?1, source_json = ?2 WHERE intent_id = ?3"
            ),
            (digest, json, id),
        )
        .await?;
    Ok(())
}

pub(super) async fn remove_source(connection: &Connection, id: i64) -> TestResult<()> {
    connection
        .execute(
            &format!(
                "UPDATE {PROOF_TABLE} SET source_digest = NULL, source_json = NULL WHERE intent_id = ?1"
            ),
            (id,),
        )
        .await?;
    Ok(())
}

pub(super) async fn source_digest(connection: &Connection, id: i64) -> TestResult<Option<String>> {
    let mut rows = connection
        .query(
            &format!("SELECT source_digest FROM {PROOF_TABLE} WHERE intent_id = ?1"),
            (id,),
        )
        .await?;
    let row = rows.next().await?.ok_or("missing proof")?;
    Ok(row.get::<Option<String>>(0)?)
}

pub(super) async fn publish_challenge(connection: &Connection, id: i64) -> TestResult<()> {
    let bytes = i64::try_from(CHALLENGE_JSON.len()).map_err(|_| "challenge json length")?;
    let values = turso::params![
        "/opt/velnor/source-proof/control/challenge.json",
        11,
        bytes,
        0,
        0,
        292,
        12,
        0,
        1,
        0,
        1,
        "0123456789abcdef",
        "fedcba9876543210",
        DIGEST,
        1,
        1,
        id
    ];
    let assignments = FILE_COLUMNS
        .iter()
        .enumerate()
        .map(|(index, column)| format!("{column} = ?{}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    connection
        .execute(
            &format!(
                "UPDATE {PROOF_TABLE} SET challenge_state = 'published', {assignments} WHERE intent_id = ?17"
            ),
            values,
        )
        .await?;
    Ok(())
}

pub(super) async fn clear_challenge_file(connection: &Connection, id: i64) -> TestResult<()> {
    let assignments = FILE_COLUMNS
        .iter()
        .map(|column| format!("{column} = NULL"))
        .collect::<Vec<_>>()
        .join(", ");
    connection
        .execute(
            &format!(
                "UPDATE {PROOF_TABLE} SET {assignments}, challenge_state = 'abandoned' WHERE intent_id = ?1"
            ),
            (id,),
        )
        .await?;
    Ok(())
}

pub(super) async fn challenge_state(
    connection: &Connection,
    id: i64,
) -> TestResult<Option<String>> {
    let mut rows = connection
        .query(
            &format!("SELECT challenge_state FROM {PROOF_TABLE} WHERE intent_id = ?1"),
            (id,),
        )
        .await?;
    let row = rows.next().await?.ok_or("missing proof")?;
    Ok(row.get::<Option<String>>(0)?)
}

pub(super) async fn challenge_file_inode(
    connection: &Connection,
    id: i64,
) -> TestResult<Option<i64>> {
    let mut rows = connection
        .query(
            &format!("SELECT challenge_file_inode FROM {PROOF_TABLE} WHERE intent_id = ?1"),
            (id,),
        )
        .await?;
    let row = rows.next().await?.ok_or("missing proof")?;
    Ok(row.get::<Option<i64>>(0)?)
}

pub(super) async fn downgrade_guards_to_v52(journal: &Journal) -> TestResult<()> {
    let connection = journal.connection().await?;
    for (name, sql) in super::super::ddl::v52_proof_transition_guards() {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {name}"), ())
            .await?;
        connection.execute(sql, ()).await?;
    }
    Ok(())
}

pub(in crate::journal::schema) async fn downgrade_to_v52(journal: &Journal) -> TestResult<()> {
    downgrade_guards_to_v52(journal).await?;
    let connection = journal.connection().await?;
    crate::journal::schema::post_jit_effect_fences::downgrade_fences_to_v36(&connection).await?;
    connection.execute("PRAGMA user_version = 52", ()).await?;
    Ok(())
}
