//! Strict checks for request and physical runner tables.

use crate::error::HostError;

mod rows;

use super::super::read_table_columns;

const AUTH_TABLE: &str = "connection_request_authorizations";
const JIT_TABLE: &str = "connection_runner_jits";

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    validate_auth_table(conn).await?;
    validate_jit_table(conn).await?;
    validate_indexes(conn).await?;
    rows::validate_auth_rows(conn).await?;
    rows::validate_jit_rows(conn).await
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

async fn validate_jit_table(conn: &turso::Connection) -> Result<(), HostError> {
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

async fn validate_indexes(conn: &turso::Connection) -> Result<(), HostError> {
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
