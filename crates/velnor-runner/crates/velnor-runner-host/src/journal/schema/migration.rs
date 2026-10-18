//! Versioned journal migrations.

use crate::error::HostError;

use super::{
    connection, image_import, remote, request_ledger, scale_set_create, source_authorization,
};

pub(super) async fn from_version_zero(conn: &turso::Connection) -> Result<(), HostError> {
    crate::journal_schema::bootstrap(conn).await?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    ensure_legacy_columns(conn).await?;
    ensure_completion_objects(conn).await?;
    super::validate_current_schema(conn).await?;
    conn.execute(
        "UPDATE intents SET state = CASE WHEN state = 'failed' THEN 'uncertain' ELSE state END, cleanup_proven = 0 WHERE kind = 'launch' AND state IN ('failed', 'pending', 'uncertain')",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute("PRAGMA user_version = 1", ())
        .await
        .map_err(|_| HostError::Journal)?;
    from_version_one(conn).await
}

pub(super) async fn from_version_one(conn: &turso::Connection) -> Result<(), HostError> {
    ensure_completion_objects(conn).await?;
    crate::journal_schema::bootstrap(conn).await?;
    connection::bootstrap_connection_objects(conn).await?;
    remote::bootstrap(conn).await?;
    crate::journal_schema::install_revision_triggers(conn).await?;
    super::validate_current_schema(conn).await?;
    connection::validate_connection_schema(conn).await?;
    conn.execute("PRAGMA user_version = 2", ())
        .await
        .map_err(|_| HostError::Journal)?;
    from_version_two(conn).await
}

pub(super) async fn from_version_two(conn: &turso::Connection) -> Result<(), HostError> {
    ensure_completion_objects(conn).await?;
    connection::bootstrap_connection_objects(conn).await?;
    remote::bootstrap(conn).await?;
    crate::journal_schema::install_revision_triggers(conn).await?;
    super::validate_current_schema(conn).await?;
    connection::validate_connection_schema(conn).await?;
    remote::validate(conn).await?;
    conn.execute("PRAGMA user_version = 3", ())
        .await
        .map_err(|_| HostError::Journal)?;
    from_version_three(conn).await
}

pub(super) async fn from_version_three(conn: &turso::Connection) -> Result<(), HostError> {
    ensure_completion_objects(conn).await?;
    connection::bootstrap_connection_objects(conn).await?;
    remote::bootstrap(conn).await?;
    crate::journal_schema::install_revision_triggers(conn).await?;
    connection::validate_connection_schema(conn).await?;
    remote::validate(conn).await?;
    super::validate_current_schema(conn).await?;
    conn.execute("PRAGMA user_version = 4", ())
        .await
        .map_err(|_| HostError::Journal)?;
    from_version_four(conn).await
}

pub(super) async fn from_version_four(conn: &turso::Connection) -> Result<(), HostError> {
    image_import::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 5", ())
        .await
        .map_err(|_| HostError::Journal)?;
    from_version_five(conn).await
}

pub(super) async fn from_version_five(conn: &turso::Connection) -> Result<(), HostError> {
    scale_set_create::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 6", ())
        .await
        .map_err(|_| HostError::Journal)?;
    from_version_six(conn).await
}

pub(super) async fn from_version_six(conn: &turso::Connection) -> Result<(), HostError> {
    image_import::upgrade_identity(conn).await?;
    conn.execute("PRAGMA user_version = 7", ())
        .await
        .map_err(|_| HostError::Journal)?;
    from_version_seven(conn).await
}

pub(super) async fn from_version_seven(conn: &turso::Connection) -> Result<(), HostError> {
    source_authorization::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 8", ())
        .await
        .map_err(|_| HostError::Journal)?;
    from_version_eight(conn).await
}

pub(super) async fn from_version_eight(conn: &turso::Connection) -> Result<(), HostError> {
    source_authorization::upgrade_v8(conn).await?;
    conn.execute("PRAGMA user_version = 9", ())
        .await
        .map_err(|_| HostError::Journal)?;
    from_version_nine(conn).await
}

pub(super) async fn from_version_nine(conn: &turso::Connection) -> Result<(), HostError> {
    request_ledger::bootstrap(conn).await?;
    conn.execute("PRAGMA user_version = 11", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn from_version_ten(conn: &turso::Connection) -> Result<(), HostError> {
    request_ledger::upgrade_cleaned_runner_state(conn).await?;
    conn.execute("PRAGMA user_version = 11", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn ensure_completion_objects(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = super::read_table_columns(conn, "intents").await?;
    for (column, definition) in [
        ("scale_set_id", "INTEGER"),
        ("runner_request_id", "INTEGER"),
        ("runner_name", "TEXT"),
        ("acquire_attempted", "INTEGER NOT NULL DEFAULT 0"),
        ("acquire_resolved", "INTEGER NOT NULL DEFAULT 0"),
        ("acquired", "INTEGER NOT NULL DEFAULT 0"),
        ("jit_requested", "INTEGER NOT NULL DEFAULT 0"),
        ("docker_engine_id", "TEXT"),
        ("launch_phase", "TEXT"),
        ("launch_id", "TEXT"),
        ("assignment_key", "TEXT"),
        ("seed_generation_id", "TEXT"),
        ("runner_completed", "INTEGER NOT NULL DEFAULT 0"),
        ("worker_cleanup_proven", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !columns.contains_key(column) {
            conn.execute(
                &format!("ALTER TABLE intents ADD COLUMN {column} {definition}"),
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        }
    }
    for statement in [
        "CREATE TABLE IF NOT EXISTS completion_cleanup (intent_id INTEGER PRIMARY KEY REFERENCES intents(id), scale_set_id INTEGER NOT NULL, runner_request_id INTEGER NOT NULL, runner_id INTEGER NOT NULL, runner_name TEXT NOT NULL, runner_absent INTEGER NOT NULL DEFAULT 0 CHECK (runner_absent IN (0, 1)), attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0), claim_generation INTEGER NOT NULL DEFAULT 0 CHECK (claim_generation >= 0), retry_after INTEGER NOT NULL DEFAULT 0 CHECK (retry_after >= 0), lease_until INTEGER NOT NULL DEFAULT 0 CHECK (lease_until >= 0), UNIQUE (scale_set_id, runner_request_id), UNIQUE (scale_set_id, runner_id), UNIQUE (scale_set_id, runner_name))",
        "CREATE TABLE IF NOT EXISTS completion_inbox (scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), message_id INTEGER NOT NULL CHECK (message_id >= 0), raw_body TEXT NOT NULL CHECK (length(CAST(raw_body AS BLOB)) BETWEEN 1 AND 262144), attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0), retry_after INTEGER NOT NULL DEFAULT 0 CHECK (retry_after >= 0), PRIMARY KEY (scale_set_id, message_id))",
        "CREATE TABLE IF NOT EXISTS launch_recovery (intent_id INTEGER PRIMARY KEY REFERENCES intents(id), generation INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0), attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0), retry_after INTEGER NOT NULL DEFAULT 0 CHECK (retry_after >= 0), lease_until INTEGER NOT NULL DEFAULT 0 CHECK (lease_until >= 0))",
        "CREATE INDEX IF NOT EXISTS launch_recovery_due ON launch_recovery(retry_after, lease_until, intent_id)",
        "CREATE TABLE IF NOT EXISTS guest_probe_owner (id INTEGER PRIMARY KEY CHECK (id = 1), owner_token TEXT NOT NULL CHECK (length(owner_token) = 32 AND owner_token NOT GLOB '*[^0-9a-f]*'))",
        "CREATE INDEX IF NOT EXISTS completion_inbox_due ON completion_inbox(retry_after, scale_set_id, message_id)",
        "CREATE UNIQUE INDEX IF NOT EXISTS intents_completion_request ON intents(kind, scale_set_id, runner_request_id) WHERE kind = 'launch' AND scale_set_id IS NOT NULL AND runner_request_id IS NOT NULL AND cleanup_proven = 0",
        "CREATE UNIQUE INDEX IF NOT EXISTS intents_completion_name ON intents(kind, scale_set_id, runner_name) WHERE kind = 'launch' AND scale_set_id IS NOT NULL AND runner_name IS NOT NULL AND cleanup_proven = 0",
    ] {
        conn.execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    ensure_runner_absent(conn).await
}

async fn ensure_legacy_columns(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = super::read_table_columns(conn, "intents").await?;
    for (column, definition) in [("dind_id", "TEXT"), ("worker_volume", "TEXT")] {
        if !columns.contains_key(column) {
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

async fn ensure_runner_absent(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = super::read_table_columns(conn, "completion_cleanup").await?;
    if !columns.contains_key("runner_absent") {
        conn.execute(
            "ALTER TABLE completion_cleanup ADD COLUMN runner_absent INTEGER NOT NULL DEFAULT 0 CHECK (runner_absent IN (0, 1))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}
