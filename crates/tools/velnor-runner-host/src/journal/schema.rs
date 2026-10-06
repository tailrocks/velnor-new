//! Versioned journal initialization and conservative legacy-row migration.

use std::collections::HashSet;

use crate::error::HostError;

const JOURNAL_VERSION: i64 = 1;
const CURRENT_COLUMNS: [&str; 9] = [
    "id",
    "kind",
    "subject",
    "state",
    "docker_id",
    "github_runner_id",
    "cleanup_proven",
    "dind_id",
    "worker_volume",
];

pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    conn.execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = bootstrap_transaction(conn).await;
    let end = if result.is_ok() {
        conn.execute("COMMIT", ()).await
    } else {
        conn.execute("ROLLBACK", ()).await
    };
    end.map_err(|_| HostError::Journal)?;
    result
}

async fn bootstrap_transaction(conn: &turso::Connection) -> Result<(), HostError> {
    match journal_version(conn).await? {
        0 => migrate_version_zero(conn).await,
        JOURNAL_VERSION => validate_current_schema(conn).await,
        _ => Err(HostError::Journal),
    }
}

async fn journal_version(conn: &turso::Connection) -> Result<i64, HostError> {
    let mut rows = conn
        .query("PRAGMA user_version", ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)
}

async fn migrate_version_zero(conn: &turso::Connection) -> Result<(), HostError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    ensure_legacy_columns(conn).await?;
    validate_current_schema(conn).await?;
    conn.execute(
        "UPDATE intents SET state = CASE WHEN state = 'failed' THEN 'uncertain' ELSE state END, cleanup_proven = 0 WHERE kind = 'launch' AND state IN ('failed', 'pending', 'uncertain')",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute("PRAGMA user_version = 1", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn ensure_legacy_columns(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_columns(conn).await?;
    for (column, definition) in [("dind_id", "TEXT"), ("worker_volume", "TEXT")] {
        if !columns.contains(column) {
            conn.execute(
                &format!("ALTER TABLE intents ADD COLUMN {column} {definition}"),
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        }
    }
    Ok(())
}

async fn validate_current_schema(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_columns(conn).await?;
    if CURRENT_COLUMNS
        .iter()
        .all(|column| columns.contains(*column))
    {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn read_columns(conn: &turso::Connection) -> Result<HashSet<String>, HostError> {
    let mut columns = HashSet::new();
    let mut rows = conn
        .query("PRAGMA table_info(intents)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        columns.insert(row.get::<String>(1).map_err(|_| HostError::Journal)?);
    }
    Ok(columns)
}
