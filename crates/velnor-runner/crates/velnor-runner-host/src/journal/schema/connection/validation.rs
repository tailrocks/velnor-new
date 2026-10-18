//! Strict validation for connection, binding, and reservation schema.

use super::super::read_table_columns;
use crate::error::HostError;

pub(in crate::journal::schema) async fn validate_connection_schema(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_scope_table(conn).await?;
    validate_intents_table(conn).await?;
    validate_runtime_bindings(conn).await?;
    validate_capacity_policy_table(conn).await?;
    validate_reservations_table(conn).await?;
    validate_reservation_samples_table(conn).await?;
    validate_connection_indexes(conn).await
}

async fn validate_scope_table(conn: &turso::Connection) -> Result<(), HostError> {
    validate_connection_table(
        conn,
        "connection_scopes",
        &[
            ConnectionColumn::new("connection_id", "TEXT", true, None, 1),
            ConnectionColumn::new("identity", "TEXT", true, None, 0),
            ConnectionColumn::new("retired", "INTEGER", true, Some("0"), 0),
        ],
        &[
            "primary key",
            "check (length(connection_id) between 1 and 64 and connection_id not glob '*[^a-z0-9._-]*' and substr(connection_id, 1, 1) != '-' and substr(connection_id, -1, 1) != '-' and instr(connection_id, '..') = 0)",
            "identity text not null",
            "check (length(cast(identity as blob)) between 1 and 1048576)",
            "retired integer not null default 0 check (retired in (0, 1))",
        ],
    )
    .await
}

async fn validate_intents_table(conn: &turso::Connection) -> Result<(), HostError> {
    validate_connection_table(
        conn,
        "connection_intents",
        &[
            ConnectionColumn::new("id", "INTEGER", false, None, 1),
            ConnectionColumn::new("connection_id", "TEXT", true, None, 0),
            ConnectionColumn::new("kind", "TEXT", true, None, 0),
            ConnectionColumn::new("subject", "TEXT", true, None, 0),
            ConnectionColumn::new("state", "TEXT", true, None, 0),
            ConnectionColumn::new("cleanup_proven", "INTEGER", true, Some("0"), 0),
            ConnectionColumn::new("docker_id", "TEXT", false, None, 0),
            ConnectionColumn::new("github_runner_id", "TEXT", false, None, 0),
            ConnectionColumn::new("dind_id", "TEXT", false, None, 0),
            ConnectionColumn::new("worker_volume", "TEXT", false, None, 0),
            ConnectionColumn::new("scale_set_id", "INTEGER", false, None, 0),
            ConnectionColumn::new("runner_request_id", "INTEGER", false, None, 0),
            ConnectionColumn::new("runner_name", "TEXT", false, None, 0),
            ConnectionColumn::new("acquire_attempted", "INTEGER", true, Some("0"), 0),
            ConnectionColumn::new("acquire_resolved", "INTEGER", true, Some("0"), 0),
            ConnectionColumn::new("acquired", "INTEGER", true, Some("0"), 0),
            ConnectionColumn::new("jit_requested", "INTEGER", true, Some("0"), 0),
            ConnectionColumn::new("docker_engine_id", "TEXT", false, None, 0),
            ConnectionColumn::new("launch_phase", "TEXT", false, None, 0),
            ConnectionColumn::new("launch_id", "TEXT", false, None, 0),
            ConnectionColumn::new("assignment_key", "TEXT", false, None, 0),
            ConnectionColumn::new("seed_generation_id", "TEXT", false, None, 0),
            ConnectionColumn::new("runner_completed", "INTEGER", true, Some("0"), 0),
            ConnectionColumn::new("worker_cleanup_proven", "INTEGER", true, Some("0"), 0),
        ],
        &[
            "references connection_scopes",
            "check (state in",
            "check (cleanup_proven in",
        ],
    )
    .await
}

async fn validate_runtime_bindings(conn: &turso::Connection) -> Result<(), HostError> {
    validate_connection_table(
        conn,
        "connection_runtime_bindings",
        &[
            ConnectionColumn::new("connection_id", "TEXT", true, None, 1),
            ConnectionColumn::new("scale_set_id", "INTEGER", false, None, 0),
            ConnectionColumn::new("scale_set_name", "TEXT", false, None, 0),
            ConnectionColumn::new("set_ownership", "TEXT", false, None, 0),
            ConnectionColumn::new("engine_id", "TEXT", false, None, 0),
            ConnectionColumn::new("docker_endpoint", "TEXT", false, None, 0),
            ConnectionColumn::new("platform", "TEXT", false, None, 0),
        ],
        &[
            "primary key",
            "references connection_scopes",
            "check (set_ownership is null or set_ownership in ('created', 'adopted'))",
        ],
    )
    .await
}

async fn validate_capacity_policy_table(conn: &turso::Connection) -> Result<(), HostError> {
    validate_connection_table(
        conn,
        "host_capacity_policy",
        &[
            ConnectionColumn::new("singleton", "INTEGER", false, None, 1),
            ConnectionColumn::new("max_jobs", "INTEGER", true, None, 0),
            ConnectionColumn::new("max_cpu_millicores", "INTEGER", true, None, 0),
            ConnectionColumn::new("max_memory_bytes", "INTEGER", true, None, 0),
        ],
        &[
            "check (singleton = 1",
            "check (max_jobs > 0",
            "check (max_cpu_millicores > 0",
            "check (max_memory_bytes > 0",
        ],
    )
    .await
}

async fn validate_reservations_table(conn: &turso::Connection) -> Result<(), HostError> {
    validate_connection_table(
        conn,
        "capacity_reservations",
        &[
            ConnectionColumn::new("id", "INTEGER", false, None, 1),
            ConnectionColumn::new("intent_id", "INTEGER", true, None, 0),
            ConnectionColumn::new("connection_id", "TEXT", true, None, 0),
            ConnectionColumn::new("assignment_key", "TEXT", true, None, 0),
            ConnectionColumn::new("engine_id", "TEXT", true, None, 0),
            ConnectionColumn::new("docker_root_digest", "TEXT", true, None, 0),
            ConnectionColumn::new("binding_digest", "TEXT", true, None, 0),
            ConnectionColumn::new("sample_generation_id", "TEXT", false, None, 0),
            ConnectionColumn::new("cpu_millicores", "INTEGER", true, None, 0),
            ConnectionColumn::new("memory_bytes", "INTEGER", true, None, 0),
            ConnectionColumn::new("state", "TEXT", true, None, 0),
            ConnectionColumn::new("remote_cleanup_proven", "INTEGER", true, Some("0"), 0),
            ConnectionColumn::new("local_cleanup_proven", "INTEGER", true, Some("0"), 0),
        ],
        &[
            "references connection_intents",
            "references connection_scopes",
            "check (state in",
            "check (state != 'released' or",
        ],
    )
    .await
}

async fn validate_reservation_samples_table(conn: &turso::Connection) -> Result<(), HostError> {
    validate_connection_table(
        conn,
        "capacity_reservation_samples",
        &[
            ConnectionColumn::new("reservation_id", "INTEGER", true, None, 1),
            ConnectionColumn::new("sample_generation_id", "TEXT", true, None, 2),
            ConnectionColumn::new("purpose", "TEXT", true, None, 0),
        ],
        &[
            "references capacity_reservations",
            "sample_generation_id text not null unique",
            "check (purpose in ('initial', 'resume'))",
            "primary key (reservation_id, sample_generation_id)",
        ],
    )
    .await
}

async fn validate_connection_indexes(conn: &turso::Connection) -> Result<(), HostError> {
    validate_index(
        conn,
        "connection_intents",
        "connection_intent_live",
        true,
        true,
        &["connection_id", "kind", "subject"],
        Some("where state != 'failed' and cleanup_proven = 0"),
    )
    .await?;
    validate_index(
        conn,
        "capacity_reservations",
        "capacity_reservation_intent",
        true,
        false,
        &["intent_id"],
        None,
    )
    .await?;
    validate_index(
        conn,
        "connection_intents",
        "connection_intent_launch_id",
        true,
        true,
        &["launch_id"],
        Some("where launch_id is not null"),
    )
    .await?;
    validate_index(
        conn,
        "connection_intents",
        "connection_intent_assignment_key",
        true,
        true,
        &["connection_id", "assignment_key"],
        Some("where assignment_key is not null"),
    )
    .await?;
    validate_index(
        conn,
        "capacity_reservations",
        "capacity_reservation_assignment",
        true,
        false,
        &["connection_id", "assignment_key"],
        None,
    )
    .await?;
    Ok(())
}

struct ConnectionColumn {
    name: &'static str,
    declared_type: &'static str,
    not_null: bool,
    default_value: Option<&'static str>,
    primary_key_position: i64,
}

impl ConnectionColumn {
    const fn new(
        name: &'static str,
        declared_type: &'static str,
        not_null: bool,
        default_value: Option<&'static str>,
        primary_key_position: i64,
    ) -> Self {
        Self {
            name,
            declared_type,
            not_null,
            default_value,
            primary_key_position,
        }
    }
}

async fn validate_connection_table(
    conn: &turso::Connection,
    table: &str,
    expected: &[ConnectionColumn],
    required_sql: &[&str],
) -> Result<(), HostError> {
    let actual = read_table_columns(conn, table).await?;
    if actual.len() != expected.len() {
        return Err(HostError::Journal);
    }
    for expected_column in expected {
        let Some(actual_column) = actual.get(expected_column.name) else {
            return Err(HostError::Journal);
        };
        if !actual_column
            .declared_type
            .trim()
            .eq_ignore_ascii_case(expected_column.declared_type)
            || actual_column.not_null != expected_column.not_null
            || actual_column.default_value.as_deref() != expected_column.default_value
            || actual_column.primary_key_position != expected_column.primary_key_position
        {
            return Err(HostError::Journal);
        }
    }
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [table],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let sql: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    let normalized = sql.to_ascii_lowercase().replace(char::is_whitespace, "");
    if required_sql
        .iter()
        .any(|fragment| !normalized.contains(&fragment.replace(char::is_whitespace, "")))
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_index(
    conn: &turso::Connection,
    table: &str,
    index: &str,
    unique: bool,
    partial: bool,
    expected_columns: &[&str],
    expected_predicate: Option<&str>,
) -> Result<(), HostError> {
    let mut schema_rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'index' AND tbl_name = ?1 AND name = ?2",
            (table, index),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let index_sql: String = schema_rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if expected_predicate.is_some_and(|predicate| {
        !index_sql
            .to_ascii_lowercase()
            .replace(char::is_whitespace, "")
            .contains(&predicate.replace(char::is_whitespace, ""))
    }) {
        return Err(HostError::Journal);
    }
    let mut rows = conn
        .query(&format!("PRAGMA index_list({table})"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut found = false;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name: String = row.get(1).map_err(|_| HostError::Journal)?;
        if name == index {
            let actual_unique = row.get::<i64>(2).map_err(|_| HostError::Journal)? != 0;
            let actual_partial = row.get::<i64>(4).map_err(|_| HostError::Journal)? != 0;
            if actual_unique != unique || actual_partial != partial {
                return Err(HostError::Journal);
            }
            found = true;
            break;
        }
    }
    if !found {
        return Err(HostError::Journal);
    }
    let mut rows = conn
        .query(&format!("PRAGMA index_info({index})"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut actual_columns = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        actual_columns.push(row.get::<String>(2).map_err(|_| HostError::Journal)?);
    }
    if actual_columns
        .iter()
        .map(String::as_str)
        .ne(expected_columns.iter().copied())
    {
        return Err(HostError::Journal);
    }
    Ok(())
}
