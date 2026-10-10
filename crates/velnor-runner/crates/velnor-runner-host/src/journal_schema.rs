//! Durable journal schema and small idempotent migrations.

use turso::Connection;
use uuid::Uuid;

use crate::error::HostError;

mod resource_probe;

/// Run the extended-schema statements inside the caller's transaction.
///
/// The journal bootstrap owns the transaction so a validation failure rolls
/// back these statements together with the versioned schema migration.
pub(super) async fn bootstrap(connection: &Connection) -> Result<(), HostError> {
    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let lifecycle_flags_present = lifecycle_flags_present(connection).await?;
    ensure_column(connection, "dind_id", "TEXT").await?;
    ensure_column(connection, "launch_id", "TEXT").await?;
    ensure_column(connection, "assignment_key", "TEXT").await?;
    ensure_column(connection, "seed_generation_id", "TEXT").await?;
    ensure_column(
        connection,
        "acquire_attempted",
        "INTEGER NOT NULL DEFAULT 0",
    )
    .await?;
    ensure_column(connection, "acquire_resolved", "INTEGER NOT NULL DEFAULT 0").await?;
    ensure_column(connection, "acquired", "INTEGER NOT NULL DEFAULT 0").await?;
    ensure_column(connection, "jit_requested", "INTEGER NOT NULL DEFAULT 0").await?;
    ensure_column(connection, "runner_completed", "INTEGER NOT NULL DEFAULT 0").await?;
    ensure_column(
        connection,
        "worker_cleanup_proven",
        "INTEGER NOT NULL DEFAULT 0",
    )
    .await?;
    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS journal_meta (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), instance_id TEXT NOT NULL, engine_id TEXT, revision INTEGER NOT NULL DEFAULT 0, lineage_pinned INTEGER NOT NULL DEFAULT 0)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    resource_probe::bootstrap(connection).await?;
    // The versioned schema owns `completion_cleanup`; its shape carries the
    // scale-set identity columns and uniqueness guards this package never had.
    let instance_id = Uuid::new_v4().simple().to_string();
    connection
        .execute(
            "INSERT OR IGNORE INTO journal_meta (singleton, instance_id) VALUES (1, ?1)",
            [instance_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    ensure_meta_column(connection, "revision", "INTEGER NOT NULL DEFAULT 0").await?;
    ensure_meta_column(connection, "lineage_pinned", "INTEGER NOT NULL DEFAULT 0").await?;
    if !lifecycle_flags_present {
        quarantine_legacy_lifecycle(connection).await?;
    }
    // Revision triggers install separately once `completion_cleanup` exists.
    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS unique_launch_id ON intents(launch_id) WHERE launch_id IS NOT NULL",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP INDEX IF EXISTS unique_assignment_key", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS unique_active_assignment_key ON intents(assignment_key) WHERE assignment_key IS NOT NULL AND cleanup_proven = 0",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn quarantine_legacy_lifecycle(connection: &Connection) -> Result<(), HostError> {
    drop_revision_triggers(connection).await?;
    let changed = connection
        .execute(
            "UPDATE intents SET acquire_attempted = 1, jit_requested = 1 WHERE kind = 'launch' AND cleanup_proven = 0",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed > 0 {
        connection
            .execute(
                "UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn lifecycle_flags_present(connection: &Connection) -> Result<bool, HostError> {
    for name in [
        "acquire_attempted",
        "acquire_resolved",
        "acquired",
        "jit_requested",
    ] {
        if !has_column(connection, name).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn drop_revision_triggers(connection: &Connection) -> Result<(), HostError> {
    for name in [
        "intents_revision_insert",
        "intents_revision_update",
        "intents_revision_delete",
        "completion_cleanup_revision_insert",
        "completion_cleanup_revision_update",
        "completion_cleanup_revision_delete",
        "resource_probe_revision_insert",
        "resource_probe_revision_update",
        "resource_probe_revision_delete",
        "journal_lineage_revision",
    ] {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

/// Install the revision triggers once every journal table exists.
///
/// Runs after the versioned schema ensures `completion_cleanup`; creating a
/// trigger on a missing table fails the whole bootstrap transaction.
pub(super) async fn install_revision_triggers(connection: &Connection) -> Result<(), HostError> {
    for statement in [
        "DROP TRIGGER IF EXISTS journal_lineage_revision",
        "CREATE TRIGGER IF NOT EXISTS intents_revision_insert AFTER INSERT ON intents BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER IF NOT EXISTS intents_revision_update AFTER UPDATE ON intents BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER IF NOT EXISTS intents_revision_delete AFTER DELETE ON intents BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER IF NOT EXISTS completion_cleanup_revision_insert AFTER INSERT ON completion_cleanup BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER IF NOT EXISTS completion_cleanup_revision_update AFTER UPDATE ON completion_cleanup BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER IF NOT EXISTS completion_cleanup_revision_delete AFTER DELETE ON completion_cleanup BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER IF NOT EXISTS resource_probe_revision_insert AFTER INSERT ON resource_probe_operations BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER IF NOT EXISTS resource_probe_revision_update AFTER UPDATE ON resource_probe_operations BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER IF NOT EXISTS resource_probe_revision_delete AFTER DELETE ON resource_probe_operations BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER journal_lineage_revision AFTER UPDATE OF instance_id, engine_id, lineage_pinned ON journal_meta WHEN OLD.instance_id IS NOT NEW.instance_id OR OLD.engine_id IS NOT NEW.engine_id OR OLD.lineage_pinned IS NOT NEW.lineage_pinned BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
    ] {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
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

pub(super) async fn bind_engine(
    connection: &Connection,
    expected_engine: &str,
) -> Result<(), HostError> {
    if !engine_id_valid(expected_engine) {
        return Err(HostError::Journal);
    }
    if let Some(current) = engine_id_optional(connection).await? {
        return if current == expected_engine {
            Ok(())
        } else {
            Err(HostError::Journal)
        };
    }
    let changed = connection
        .execute(
            "UPDATE journal_meta SET engine_id = ?1 WHERE singleton = 1 AND engine_id IS NULL",
            [expected_engine],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        (engine_id_optional(connection).await? == Some(expected_engine.to_owned()))
            .then_some(())
            .ok_or(HostError::Journal)
    }
}

fn engine_id_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
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

pub(super) async fn revision(connection: &Connection) -> Result<u64, HostError> {
    let mut rows = connection
        .query("SELECT revision FROM journal_meta WHERE singleton = 1", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let revision: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    u64::try_from(revision).map_err(|_| HostError::Journal)
}

pub(super) async fn engine_id_optional(
    connection: &Connection,
) -> Result<Option<String>, HostError> {
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

async fn ensure_meta_column(
    connection: &Connection,
    name: &str,
    kind: &str,
) -> Result<(), HostError> {
    if has_column_named(connection, "journal_meta", name).await? {
        return Ok(());
    }
    let statement = format!("ALTER TABLE journal_meta ADD COLUMN {name} {kind}");
    connection
        .execute(&statement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn has_column_named(
    connection: &Connection,
    table: &str,
    name: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(&format!("PRAGMA table_info({table})"), ())
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
