//! Durable journal schema and small idempotent migrations.

use turso::Connection;
use uuid::Uuid;

use crate::error::HostError;

pub(super) async fn bootstrap(connection: &Connection) -> Result<(), HostError> {
    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    ensure_column(connection, "dind_id", "TEXT").await?;
    ensure_column(connection, "launch_id", "TEXT").await?;
    ensure_column(connection, "assignment_key", "TEXT").await?;
    ensure_column(connection, "seed_generation_id", "TEXT").await?;
    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS journal_meta (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), instance_id TEXT NOT NULL, engine_id TEXT)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let instance_id = Uuid::new_v4().simple().to_string();
    connection
        .execute(
            "INSERT OR IGNORE INTO journal_meta (singleton, instance_id) VALUES (1, ?1)",
            [instance_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS unique_launch_id ON intents(launch_id) WHERE launch_id IS NOT NULL",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS unique_assignment_key ON intents(assignment_key) WHERE assignment_key IS NOT NULL",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn instance_id(connection: &Connection) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT instance_id FROM journal_meta WHERE singleton = 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    row.get(0).map_err(|_| HostError::Journal)
}

pub(super) async fn bind_engine(connection: &Connection, engine_id: &str) -> Result<(), HostError> {
    if !engine_id_valid(engine_id) {
        return Err(HostError::Journal);
    }
    let changed = connection
        .execute(
            "UPDATE journal_meta SET engine_id = COALESCE(engine_id, ?1) WHERE singleton = 1 AND (engine_id IS NULL OR engine_id = ?1)",
            [engine_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn engine_id_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

pub(super) async fn engine_id(connection: &Connection) -> Result<String, HostError> {
    let mut rows = connection
        .query("SELECT engine_id FROM journal_meta WHERE singleton = 1", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    row.get(0).map_err(|_| HostError::Journal)
}

async fn ensure_column(connection: &Connection, name: &str, kind: &str) -> Result<(), HostError> {
    if has_column(connection, name).await? {
        return Ok(());
    }
    let statement = format!("ALTER TABLE intents ADD COLUMN {name} {kind}");
    connection
        .execute(&statement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn has_column(connection: &Connection, name: &str) -> Result<bool, HostError> {
    let mut rows = connection
        .query("PRAGMA table_info(intents)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let column: String = row.get(1).map_err(|_| HostError::Journal)?;
        if column == name {
            return Ok(true);
        }
    }
    Ok(false)
}
