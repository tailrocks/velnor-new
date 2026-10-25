//! Remote launch completion tables keyed by immutable repository connection.

use crate::error::HostError;

use super::read_table_columns;

pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    for statement in [
        "CREATE TABLE IF NOT EXISTS connection_completion_cleanup (intent_id INTEGER PRIMARY KEY REFERENCES connection_intents(id), connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), runner_request_id INTEGER NOT NULL CHECK (runner_request_id > 0), runner_id INTEGER NOT NULL CHECK (runner_id > 0), runner_name TEXT NOT NULL, runner_absent INTEGER NOT NULL DEFAULT 0 CHECK (runner_absent IN (0, 1)), attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0), claim_generation INTEGER NOT NULL DEFAULT 0 CHECK (claim_generation >= 0), retry_after INTEGER NOT NULL DEFAULT 0 CHECK (retry_after >= 0), lease_until INTEGER NOT NULL DEFAULT 0 CHECK (lease_until >= 0), UNIQUE (connection_id, scale_set_id, runner_request_id), UNIQUE (connection_id, scale_set_id, runner_id), UNIQUE (connection_id, scale_set_id, runner_name))",
        "CREATE TABLE IF NOT EXISTS connection_completion_inbox (connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), message_id INTEGER NOT NULL CHECK (message_id >= 0), raw_body TEXT NOT NULL CHECK (length(CAST(raw_body AS BLOB)) BETWEEN 1 AND 262144), attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0), retry_after INTEGER NOT NULL DEFAULT 0 CHECK (retry_after >= 0), PRIMARY KEY (connection_id, scale_set_id, message_id))",
        "CREATE INDEX IF NOT EXISTS connection_completion_due ON connection_completion_cleanup(retry_after, lease_until, connection_id, intent_id)",
        "CREATE INDEX IF NOT EXISTS connection_completion_inbox_due ON connection_completion_inbox(connection_id, retry_after, scale_set_id, message_id)",
        "CREATE TABLE IF NOT EXISTS connection_launch_recovery (intent_id INTEGER PRIMARY KEY REFERENCES connection_intents(id), connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), generation INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0), attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0), retry_after INTEGER NOT NULL DEFAULT 0 CHECK (retry_after >= 0), lease_until INTEGER NOT NULL DEFAULT 0 CHECK (lease_until >= 0))",
        "CREATE INDEX IF NOT EXISTS connection_launch_recovery_due ON connection_launch_recovery(connection_id, retry_after, lease_until, intent_id)",
    ] {
        conn.execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    validate_table(
        conn,
        "connection_completion_cleanup",
        &[
            ("intent_id", "INTEGER", false, 1),
            ("connection_id", "TEXT", true, 0),
            ("scale_set_id", "INTEGER", true, 0),
            ("runner_request_id", "INTEGER", true, 0),
            ("runner_id", "INTEGER", true, 0),
            ("runner_name", "TEXT", true, 0),
            ("runner_absent", "INTEGER", true, 0),
            ("attempts", "INTEGER", true, 0),
            ("claim_generation", "INTEGER", true, 0),
            ("retry_after", "INTEGER", true, 0),
            ("lease_until", "INTEGER", true, 0),
        ],
        &[
            "references connection_intents",
            "references connection_scopes",
            "unique (connection_id, scale_set_id, runner_request_id)",
            "unique (connection_id, scale_set_id, runner_id)",
            "unique (connection_id, scale_set_id, runner_name)",
        ],
    )
    .await?;
    validate_table(
        conn,
        "connection_completion_inbox",
        &[
            ("connection_id", "TEXT", true, 1),
            ("scale_set_id", "INTEGER", true, 2),
            ("message_id", "INTEGER", true, 3),
            ("raw_body", "TEXT", true, 0),
            ("attempts", "INTEGER", true, 0),
            ("retry_after", "INTEGER", true, 0),
        ],
        &[
            "references connection_scopes",
            "primary key (connection_id, scale_set_id, message_id)",
        ],
    )
    .await?;
    validate_table(
        conn,
        "connection_launch_recovery",
        &[
            ("intent_id", "INTEGER", false, 1),
            ("connection_id", "TEXT", true, 0),
            ("generation", "INTEGER", true, 0),
            ("attempts", "INTEGER", true, 0),
            ("retry_after", "INTEGER", true, 0),
            ("lease_until", "INTEGER", true, 0),
        ],
        &[
            "references connection_intents",
            "references connection_scopes",
        ],
    )
    .await
}

async fn validate_table(
    conn: &turso::Connection,
    name: &str,
    expected: &[(&str, &str, bool, i64)],
    required_sql: &[&str],
) -> Result<(), HostError> {
    let columns = read_table_columns(conn, name).await?;
    if columns.len() != expected.len() {
        return Err(HostError::Journal);
    }
    for (column, kind, not_null, primary_key_position) in expected {
        let Some(actual) = columns.get(*column) else {
            return Err(HostError::Journal);
        };
        if !actual.declared_type.eq_ignore_ascii_case(kind)
            || actual.not_null != *not_null
            || actual.primary_key_position != *primary_key_position
        {
            return Err(HostError::Journal);
        }
    }
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [name],
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
