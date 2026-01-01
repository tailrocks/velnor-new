//! V12 logical Docker Engine binding schema and migration.

use crate::error::HostError;
use crate::journal::JournalDockerDaemonBinding;

pub(in crate::journal::schema) const TABLE_SQL: &str = "CREATE TABLE linux_launch_daemon_bindings (launch_id INTEGER PRIMARY KEY NOT NULL CHECK (launch_id > 0), endpoint TEXT NOT NULL CHECK (length(endpoint) BETWEEN 2 AND 4096), engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 1024), FOREIGN KEY (launch_id) REFERENCES intents(id))";

pub(in crate::journal::schema) async fn validate_schema(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_columns(conn).await?;
    validate_definition(conn).await?;
    validate_rows(conn).await
}

async fn validate_columns(conn: &turso::Connection) -> Result<(), HostError> {
    const COLUMNS: [(&str, &str, i64, i64); 3] = [
        ("launch_id", "INTEGER", 1, 1),
        ("endpoint", "TEXT", 1, 0),
        ("engine_id", "TEXT", 1, 0),
    ];
    let mut rows = conn
        .query("PRAGMA table_info(linux_launch_daemon_bindings)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    for (name, kind, not_null, primary_key) in COLUMNS {
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        if row.get::<String>(1).map_err(|_| HostError::Journal)? != name
            || row.get::<String>(2).map_err(|_| HostError::Journal)? != kind
            || row.get::<i64>(3).map_err(|_| HostError::Journal)? != not_null
            || row.get::<i64>(5).map_err(|_| HostError::Journal)? != primary_key
        {
            return Err(HostError::Journal);
        }
    }
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_definition(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'linux_launch_daemon_bindings'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let actual = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    if normalize_sql(&actual) != normalize_sql(TABLE_SQL)
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_rows(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT b.launch_id, b.endpoint, b.engine_id, i.kind FROM linux_launch_daemon_bindings AS b LEFT JOIN intents AS i ON i.id = b.launch_id ORDER BY b.launch_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let launch_id = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
        let endpoint = row.get::<String>(1).map_err(|_| HostError::Journal)?;
        let engine_id = row.get::<String>(2).map_err(|_| HostError::Journal)?;
        let kind = row
            .get::<Option<String>>(3)
            .map_err(|_| HostError::Journal)?;
        if launch_id <= 0 || kind.as_deref() != Some("launch") {
            return Err(HostError::Journal);
        }
        JournalDockerDaemonBinding::new(&endpoint, &engine_id)?;
    }
    Ok(())
}

fn normalize_sql(sql: &str) -> String {
    sql.chars()
        .filter(|character| !character.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}
