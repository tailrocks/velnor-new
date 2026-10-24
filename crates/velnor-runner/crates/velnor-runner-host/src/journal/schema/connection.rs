//! Connection and capacity tables with strict schema validation.

use crate::error::HostError;

use super::read_table_columns;

mod validation;
pub(super) use validation::validate_connection_schema;

pub(super) async fn upgrade_capacity_policy(conn: &turso::Connection) -> Result<(), HostError> {
    ensure_columns(
        conn,
        "host_capacity_policy",
        &[
            (
                "operational_cpu_millicores",
                "INTEGER NOT NULL DEFAULT 0 CHECK (operational_cpu_millicores >= 0)",
            ),
            (
                "operational_memory_bytes",
                "INTEGER NOT NULL DEFAULT 0 CHECK (operational_memory_bytes >= 0)",
            ),
        ],
    )
    .await
}

pub(super) async fn bootstrap_connection_objects(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    for statement in [
        "CREATE TABLE IF NOT EXISTS connection_scopes (connection_id TEXT NOT NULL PRIMARY KEY CHECK (length(connection_id) BETWEEN 1 AND 64 AND connection_id NOT GLOB '*[^a-z0-9._-]*' AND substr(connection_id, 1, 1) != '-' AND substr(connection_id, -1, 1) != '-' AND instr(connection_id, '..') = 0), identity TEXT NOT NULL CHECK (length(CAST(identity AS BLOB)) BETWEEN 1 AND 1048576), retired INTEGER NOT NULL DEFAULT 0 CHECK (retired IN (0, 1)))",
        "CREATE TABLE IF NOT EXISTS connection_intents (id INTEGER PRIMARY KEY AUTOINCREMENT, connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL CHECK (state IN ('pending', 'done', 'uncertain', 'failed')), cleanup_proven INTEGER NOT NULL DEFAULT 0 CHECK (cleanup_proven IN (0, 1)), docker_id TEXT, github_runner_id TEXT, dind_id TEXT, worker_volume TEXT, scale_set_id INTEGER, runner_request_id INTEGER, runner_name TEXT, acquire_attempted INTEGER NOT NULL DEFAULT 0 CHECK (acquire_attempted IN (0, 1)), acquire_resolved INTEGER NOT NULL DEFAULT 0 CHECK (acquire_resolved IN (0, 1)), acquired INTEGER NOT NULL DEFAULT 0 CHECK (acquired IN (0, 1)), jit_requested INTEGER NOT NULL DEFAULT 0 CHECK (jit_requested IN (0, 1)), docker_engine_id TEXT, launch_phase TEXT, launch_id TEXT, assignment_key TEXT, seed_generation_id TEXT, runner_completed INTEGER NOT NULL DEFAULT 0 CHECK (runner_completed IN (0, 1)), worker_cleanup_proven INTEGER NOT NULL DEFAULT 0 CHECK (worker_cleanup_proven IN (0, 1)))",
        "CREATE TABLE IF NOT EXISTS host_capacity_policy (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), max_jobs INTEGER NOT NULL CHECK (max_jobs > 0), max_cpu_millicores INTEGER NOT NULL CHECK (max_cpu_millicores > 0), max_memory_bytes INTEGER NOT NULL CHECK (max_memory_bytes > 0), operational_cpu_millicores INTEGER NOT NULL DEFAULT 0 CHECK (operational_cpu_millicores >= 0), operational_memory_bytes INTEGER NOT NULL DEFAULT 0 CHECK (operational_memory_bytes >= 0))",
        "CREATE TABLE IF NOT EXISTS capacity_reservations (id INTEGER PRIMARY KEY AUTOINCREMENT, intent_id INTEGER NOT NULL REFERENCES connection_intents(id), connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), assignment_key TEXT NOT NULL CHECK (length(assignment_key) BETWEEN 1 AND 1024), engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 128), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'), sample_generation_id TEXT, cpu_millicores INTEGER NOT NULL CHECK (cpu_millicores > 0), memory_bytes INTEGER NOT NULL CHECK (memory_bytes > 0), state TEXT NOT NULL CHECK (state IN ('reserved', 'uncertain', 'recovering', 'released')), remote_cleanup_proven INTEGER NOT NULL DEFAULT 0 CHECK (remote_cleanup_proven IN (0, 1)), local_cleanup_proven INTEGER NOT NULL DEFAULT 0 CHECK (local_cleanup_proven IN (0, 1)), CHECK (state != 'released' OR (remote_cleanup_proven = 1 AND local_cleanup_proven = 1)))",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_samples (reservation_id INTEGER NOT NULL REFERENCES capacity_reservations(id), sample_generation_id TEXT NOT NULL UNIQUE CHECK (length(sample_generation_id) = 32 AND sample_generation_id NOT GLOB '*[^0-9a-f]*'), purpose TEXT NOT NULL CHECK (purpose IN ('initial', 'resume')), PRIMARY KEY (reservation_id, sample_generation_id))",
        "CREATE TABLE IF NOT EXISTS connection_runtime_bindings (connection_id TEXT NOT NULL PRIMARY KEY REFERENCES connection_scopes(connection_id), scale_set_id INTEGER, scale_set_name TEXT, set_ownership TEXT CHECK (set_ownership IS NULL OR set_ownership IN ('created', 'adopted')), engine_id TEXT, docker_endpoint TEXT, platform TEXT, CHECK ((scale_set_id IS NULL AND scale_set_name IS NULL AND set_ownership IS NULL) OR (scale_set_id > 0 AND length(scale_set_name) BETWEEN 1 AND 128 AND set_ownership IS NOT NULL)), CHECK ((engine_id IS NULL AND docker_endpoint IS NULL AND platform IS NULL) OR (length(engine_id) BETWEEN 1 AND 128 AND length(docker_endpoint) BETWEEN 1 AND 4096 AND length(platform) BETWEEN 1 AND 64)))",
    ] {
        conn.execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    upgrade_capacity_policy(conn).await?;
    ensure_extension_columns(conn).await?;
    for statement in [
        "CREATE UNIQUE INDEX IF NOT EXISTS connection_intent_live ON connection_intents(connection_id, kind, subject) WHERE state != 'failed' AND cleanup_proven = 0",
        "CREATE UNIQUE INDEX IF NOT EXISTS capacity_reservation_intent ON capacity_reservations(intent_id)",
        "CREATE UNIQUE INDEX IF NOT EXISTS capacity_reservation_assignment ON capacity_reservations(connection_id, assignment_key)",
        "CREATE UNIQUE INDEX IF NOT EXISTS connection_intent_launch_id ON connection_intents(launch_id) WHERE launch_id IS NOT NULL",
        "CREATE UNIQUE INDEX IF NOT EXISTS connection_intent_assignment_key ON connection_intents(connection_id, assignment_key) WHERE assignment_key IS NOT NULL",
        "CREATE UNIQUE INDEX IF NOT EXISTS connection_intent_request ON connection_intents(connection_id, scale_set_id, runner_request_id) WHERE kind = 'launch' AND scale_set_id IS NOT NULL AND runner_request_id IS NOT NULL AND cleanup_proven = 0",
        "CREATE UNIQUE INDEX IF NOT EXISTS connection_intent_runner_name ON connection_intents(connection_id, scale_set_id, runner_name) WHERE kind = 'launch' AND scale_set_id IS NOT NULL AND runner_name IS NOT NULL AND cleanup_proven = 0",
    ] {
        conn.execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn ensure_extension_columns(conn: &turso::Connection) -> Result<(), HostError> {
    ensure_columns(
        conn,
        "connection_intents",
        &[
            ("docker_id", "TEXT"),
            ("github_runner_id", "TEXT"),
            ("dind_id", "TEXT"),
            ("worker_volume", "TEXT"),
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
        ],
    )
    .await?;
    ensure_columns(
        conn,
        "capacity_reservations",
        &[("sample_generation_id", "TEXT")],
    )
    .await?;
    backfill_reservation_samples(conn).await
}

async fn backfill_reservation_samples(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM capacity_reservations WHERE state != 'released' AND sample_generation_id IS NULL",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let unresolved_without_sample = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if unresolved_without_sample != 0 {
        return Err(HostError::Journal);
    }
    conn.execute(
        "INSERT OR IGNORE INTO capacity_reservation_samples (reservation_id, sample_generation_id, purpose) SELECT id, sample_generation_id, 'initial' FROM capacity_reservations WHERE sample_generation_id IS NOT NULL",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM capacity_reservations AS r WHERE r.sample_generation_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM capacity_reservation_samples AS s WHERE s.reservation_id = r.id AND s.sample_generation_id = r.sample_generation_id)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let missing = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if missing != 0 {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn ensure_columns(
    conn: &turso::Connection,
    table: &str,
    wanted: &[(&str, &str)],
) -> Result<(), HostError> {
    let columns = read_table_columns(conn, table).await?;
    for (column, definition) in wanted {
        if !columns.contains_key(*column) {
            conn.execute(
                &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        }
    }
    Ok(())
}
