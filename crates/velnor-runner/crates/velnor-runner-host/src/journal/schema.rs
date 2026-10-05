//! Ordered local journal schema migrations.

use crate::error::HostError;

pub(super) async fn ensure_columns(conn: &turso::Connection) -> Result<(), HostError> {
    let mut has_dind = false;
    let mut has_volume = false;
    let mut has_scale_set = false;
    let mut has_request = false;
    let mut has_runner_name = false;
    let mut has_acquire_attempted = false;
    let mut has_acquire_resolved = false;
    let mut has_acquired = false;
    let mut has_jit_requested = false;
    let mut rows = conn
        .query("PRAGMA table_info(intents)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name: String = row.get(1).map_err(|_| HostError::Journal)?;
        has_dind |= name == "dind_id";
        has_volume |= name == "worker_volume";
        has_scale_set |= name == "scale_set_id";
        has_request |= name == "runner_request_id";
        has_runner_name |= name == "runner_name";
        has_acquire_attempted |= name == "acquire_attempted";
        has_acquire_resolved |= name == "acquire_resolved";
        has_acquired |= name == "acquired";
        has_jit_requested |= name == "jit_requested";
    }
    if !has_dind {
        conn.execute("ALTER TABLE intents ADD COLUMN dind_id TEXT", ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    if !has_volume {
        conn.execute("ALTER TABLE intents ADD COLUMN worker_volume TEXT", ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    if !has_scale_set {
        conn.execute("ALTER TABLE intents ADD COLUMN scale_set_id INTEGER", ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    if !has_request {
        conn.execute(
            "ALTER TABLE intents ADD COLUMN runner_request_id INTEGER",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    if !has_runner_name {
        conn.execute("ALTER TABLE intents ADD COLUMN runner_name TEXT", ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    if !has_acquire_attempted {
        conn.execute(
            "ALTER TABLE intents ADD COLUMN acquire_attempted INTEGER NOT NULL DEFAULT 0",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    if !has_acquire_resolved {
        conn.execute(
            "ALTER TABLE intents ADD COLUMN acquire_resolved INTEGER NOT NULL DEFAULT 0",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    if !has_acquired {
        conn.execute(
            "ALTER TABLE intents ADD COLUMN acquired INTEGER NOT NULL DEFAULT 0",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    if !has_jit_requested {
        conn.execute(
            "ALTER TABLE intents ADD COLUMN jit_requested INTEGER NOT NULL DEFAULT 0",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    conn.execute(
        "CREATE TABLE IF NOT EXISTS completion_cleanup (intent_id INTEGER PRIMARY KEY REFERENCES intents(id), scale_set_id INTEGER NOT NULL, runner_request_id INTEGER NOT NULL, runner_id INTEGER NOT NULL, runner_name TEXT NOT NULL, runner_absent INTEGER NOT NULL DEFAULT 0 CHECK (runner_absent IN (0, 1)), attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0), claim_generation INTEGER NOT NULL DEFAULT 0 CHECK (claim_generation >= 0), retry_after INTEGER NOT NULL DEFAULT 0 CHECK (retry_after >= 0), lease_until INTEGER NOT NULL DEFAULT 0 CHECK (lease_until >= 0), UNIQUE (scale_set_id, runner_request_id), UNIQUE (scale_set_id, runner_id), UNIQUE (scale_set_id, runner_name))",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS completion_inbox (scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), message_id INTEGER NOT NULL CHECK (message_id >= 0), raw_body TEXT NOT NULL CHECK (length(CAST(raw_body AS BLOB)) BETWEEN 1 AND 262144), attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0), retry_after INTEGER NOT NULL DEFAULT 0 CHECK (retry_after >= 0), PRIMARY KEY (scale_set_id, message_id))",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    let mut has_runner_absent = false;
    let mut rows = conn
        .query("PRAGMA table_info(completion_cleanup)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name: String = row.get(1).map_err(|_| HostError::Journal)?;
        has_runner_absent |= name == "runner_absent";
    }
    if !has_runner_absent {
        conn.execute(
            "ALTER TABLE completion_cleanup ADD COLUMN runner_absent INTEGER NOT NULL DEFAULT 0 CHECK (runner_absent IN (0, 1))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS intents_completion_request ON intents(kind, scale_set_id, runner_request_id) WHERE kind = 'launch' AND scale_set_id IS NOT NULL AND runner_request_id IS NOT NULL AND cleanup_proven = 0",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS intents_completion_name ON intents(kind, scale_set_id, runner_name) WHERE kind = 'launch' AND scale_set_id IS NOT NULL AND runner_name IS NOT NULL AND cleanup_proven = 0",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(())
}
