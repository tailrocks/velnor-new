//! Strict checks for request and physical runner tables.

use crate::error::HostError;

mod rows;
mod v40;

use super::super::read_table_columns;

const AUTH_TABLE: &str = "connection_request_authorizations";
const JIT_TABLE: &str = "connection_runner_jits";

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    validate_schema(conn, false, ValidationPhase::Current).await
}

pub(super) async fn validate_bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    validate_schema(conn, false, ValidationPhase::LegacyBootstrap).await
}

pub(super) async fn validate_v10(conn: &turso::Connection) -> Result<(), HostError> {
    validate_schema(conn, true, ValidationPhase::VersionTenMigration).await
}

pub(super) async fn validate_v40(conn: &turso::Connection) -> Result<(), HostError> {
    v40::validate(conn).await
}

#[derive(Clone, Copy)]
enum ValidationPhase {
    LegacyBootstrap,
    VersionTenMigration,
    Current,
}

async fn validate_schema(
    conn: &turso::Connection,
    version_ten: bool,
    phase: ValidationPhase,
) -> Result<(), HostError> {
    validate_auth_table(conn).await?;
    validate_jit_table(conn, version_ten).await?;
    validate_indexes(conn, !matches!(phase, ValidationPhase::VersionTenMigration)).await?;
    rows::validate_auth_rows(conn).await?;
    match phase {
        ValidationPhase::LegacyBootstrap | ValidationPhase::VersionTenMigration => {
            rows::validate_legacy_jit_rows(conn).await
        }
        ValidationPhase::Current => rows::validate_jit_rows(conn).await,
    }
}

async fn validate_auth_table(conn: &turso::Connection) -> Result<(), HostError> {
    validate_table(
        conn,
        AUTH_TABLE,
        &[
            ("connection_id", "TEXT", true, None, 1),
            ("scale_set_id", "INTEGER", true, None, 2),
            ("request_id", "INTEGER", true, None, 3),
            ("message_id", "INTEGER", true, None, 0),
            ("policy_mode", "TEXT", true, None, 0),
            ("authority_model", "TEXT", true, None, 0),
            ("binding_digest", "TEXT", true, None, 0),
            ("source_digest", "TEXT", true, None, 0),
            ("source_json", "TEXT", true, None, 0),
            ("acquire_state", "TEXT", true, None, 0),
            ("acquire_response_json", "TEXT", false, None, 0),
            ("acquire_response_digest", "TEXT", false, None, 0),
            ("started_message_id", "INTEGER", false, None, 0),
            ("started_event_digest", "TEXT", false, None, 0),
            ("started_job_id", "TEXT", false, None, 0),
            ("started_runner_id", "INTEGER", false, None, 0),
            ("started_runner_name", "TEXT", false, None, 0),
            ("completed_message_id", "INTEGER", false, None, 0),
            ("completed_event_digest", "TEXT", false, None, 0),
            ("completed_job_id", "TEXT", false, None, 0),
            ("completed_result", "TEXT", false, None, 0),
            ("completed_runner_id", "INTEGER", false, None, 0),
            ("completed_runner_name", "TEXT", false, None, 0),
            ("completed_intent_id", "INTEGER", false, None, 0),
        ],
        &[
            "primary key",
            "references connection_scopes",
            "references connection_intents",
            "acquire_state in",
            "created_empty_exclusive_admin_session_v1",
        ],
    )
    .await
}

async fn validate_jit_table(conn: &turso::Connection, version_ten: bool) -> Result<(), HostError> {
    let completion_check = if version_ten {
        "check (state not in ('completed', 'cleaned') or completed_request_id is not null)"
    } else {
        "check (state != 'completed' or completed_request_id is not null)"
    };
    validate_table(
        conn,
        JIT_TABLE,
        &[
            ("intent_id", "INTEGER", false, None, 1),
            ("connection_id", "TEXT", true, None, 0),
            ("scale_set_id", "INTEGER", true, None, 0),
            ("witness_request_id", "INTEGER", true, None, 0),
            ("requested_name", "TEXT", true, None, 0),
            ("state", "TEXT", true, None, 0),
            ("runner_id", "INTEGER", false, None, 0),
            ("runner_name", "TEXT", false, None, 0),
            ("started_request_id", "INTEGER", false, None, 0),
            ("completed_request_id", "INTEGER", false, None, 0),
        ],
        &[
            "references connection_intents",
            "references connection_scopes",
            "foreign key",
            "witness_request_id",
            "state in",
            "cleaned",
            completion_check,
        ],
    )
    .await
}

async fn validate_table(
    conn: &turso::Connection,
    table: &str,
    expected: &[(&str, &str, bool, Option<&str>, i64)],
    required_sql: &[&str],
) -> Result<(), HostError> {
    let columns = read_table_columns(conn, table).await?;
    if columns.len() != expected.len() {
        return Err(HostError::Journal);
    }
    for (name, kind, not_null, default, primary_key) in expected {
        let Some(column) = columns.get(*name) else {
            return Err(HostError::Journal);
        };
        if !column.declared_type.trim().eq_ignore_ascii_case(kind)
            || column.not_null != *not_null
            || column.default_value.as_deref() != *default
            || column.primary_key_position != *primary_key
        {
            return Err(HostError::Journal);
        }
    }
    let sql = table_sql(conn, table).await?;
    let normalized = sql.to_ascii_lowercase();
    if required_sql.iter().any(|part| !normalized.contains(part)) {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn table_sql(conn: &turso::Connection, table: &str) -> Result<String, HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [table],
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

async fn validate_indexes(
    conn: &turso::Connection,
    validate_partial_sql: bool,
) -> Result<(), HostError> {
    validate_index_catalog(conn).await?;
    if validate_partial_sql {
        validate_partial_index_sql(conn).await?;
    }
    Ok(())
}

async fn validate_index_catalog(conn: &turso::Connection) -> Result<(), HostError> {
    for (table, name, unique, columns) in [
        (
            AUTH_TABLE,
            "connection_request_authorization_state",
            false,
            vec![
                "connection_id",
                "scale_set_id",
                "acquire_state",
                "request_id",
            ],
        ),
        (
            AUTH_TABLE,
            "connection_request_completion_intent",
            false,
            vec!["connection_id", "scale_set_id", "completed_intent_id"],
        ),
        (
            JIT_TABLE,
            "connection_runner_jit_id",
            true,
            vec!["connection_id", "scale_set_id", "runner_id"],
        ),
        (
            JIT_TABLE,
            "connection_runner_jit_name",
            true,
            vec!["connection_id", "scale_set_id", "runner_name"],
        ),
        (
            JIT_TABLE,
            "connection_runner_jit_witness",
            true,
            vec!["connection_id", "scale_set_id", "witness_request_id"],
        ),
        (
            JIT_TABLE,
            "connection_runner_jit_started_request",
            true,
            vec!["connection_id", "scale_set_id", "started_request_id"],
        ),
        (
            JIT_TABLE,
            "connection_runner_jit_completed_request",
            true,
            vec!["connection_id", "scale_set_id", "completed_request_id"],
        ),
    ] {
        let mut rows = conn
            .query(&format!("PRAGMA index_list({table})"), ())
            .await
            .map_err(|_| HostError::Journal)?;
        let mut found = false;
        while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
            if row.get::<String>(1).map_err(|_| HostError::Journal)? != name {
                continue;
            }
            let actual_unique = row.get::<i64>(2).map_err(|_| HostError::Journal)? != 0;
            if actual_unique != unique {
                return Err(HostError::Journal);
            }
            found = true;
        }
        if !found
            || index_columns(conn, name).await?
                != columns.into_iter().map(str::to_owned).collect::<Vec<_>>()
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

async fn validate_partial_index_sql(conn: &turso::Connection) -> Result<(), HostError> {
    for (name, expected) in [
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
            "CREATE UNIQUE INDEX connection_runner_jit_witness ON connection_runner_jits(connection_id, scale_set_id, witness_request_id) WHERE state != 'cleaned'",
        ),
        (
            "connection_runner_jit_started_request",
            "CREATE UNIQUE INDEX connection_runner_jit_started_request ON connection_runner_jits(connection_id, scale_set_id, started_request_id) WHERE started_request_id IS NOT NULL",
        ),
        (
            "connection_runner_jit_completed_request",
            "CREATE UNIQUE INDEX connection_runner_jit_completed_request ON connection_runner_jits(connection_id, scale_set_id, completed_request_id) WHERE completed_request_id IS NOT NULL",
        ),
    ] {
        let mut rows = conn
            .query(
                "SELECT sql FROM sqlite_schema WHERE type='index' AND name=?1",
                (name,),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let actual: String = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get(0)
            .map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || super::super::sql_normalization::normalize(&actual)
                != super::super::sql_normalization::normalize(expected)
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

async fn index_columns(conn: &turso::Connection, name: &str) -> Result<Vec<String>, HostError> {
    let mut rows = conn
        .query(&format!("PRAGMA index_info({name})"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        columns.push(row.get(2).map_err(|_| HostError::Journal)?);
    }
    Ok(columns)
}
