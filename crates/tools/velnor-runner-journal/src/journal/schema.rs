//! Versioned journal initialization and conservative legacy-row migration.

use crate::error::HostError;

const JOURNAL_VERSION: i64 = 11;

mod validation;
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
    let version = journal_version(conn).await?;
    if version == JOURNAL_VERSION {
        return validation::validate_current_schema(conn).await;
    }
    match version {
        0 => {
            migrate_version_zero(conn).await?;
            migrate_version_one(conn).await?;
            migrate_version_two(conn).await?;
            migrate_version_three(conn).await?;
            migrate_version_four(conn).await?;
            migrate_version_five(conn).await?;
            migrate_version_six(conn).await?;
            migrate_version_nine(conn).await?;
        }
        1 => {
            migrate_version_one(conn).await?;
            migrate_version_two(conn).await?;
            migrate_version_three(conn).await?;
            migrate_version_four(conn).await?;
            migrate_version_five(conn).await?;
            migrate_version_six(conn).await?;
            migrate_version_nine(conn).await?;
        }
        2 => {
            migrate_version_two(conn).await?;
            migrate_version_three(conn).await?;
            migrate_version_four(conn).await?;
            migrate_version_five(conn).await?;
            migrate_version_six(conn).await?;
            migrate_version_nine(conn).await?;
        }
        3 => {
            migrate_version_three(conn).await?;
            migrate_version_four(conn).await?;
            migrate_version_five(conn).await?;
            migrate_version_six(conn).await?;
            migrate_version_nine(conn).await?;
        }
        4 => {
            migrate_version_four(conn).await?;
            migrate_version_five(conn).await?;
            migrate_version_six(conn).await?;
            migrate_version_nine(conn).await?;
        }
        5 => {
            migrate_version_five(conn).await?;
            migrate_version_six(conn).await?;
            migrate_version_nine(conn).await?;
        }
        6 => {
            migrate_version_six(conn).await?;
            migrate_version_nine(conn).await?;
        }
        7 => {
            migrate_version_seven(conn).await?;
            migrate_version_eight(conn).await?;
            migrate_version_nine(conn).await?;
        }
        8 => {
            migrate_version_eight(conn).await?;
            migrate_version_nine(conn).await?;
        }
        9 => migrate_version_nine(conn).await?,
        10 => {}
        _ => return Err(HostError::Journal),
    }
    migrate_version_ten(conn).await
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
    validation::validate_intent_schema(conn).await?;
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

async fn migrate_version_one(conn: &turso::Connection) -> Result<(), HostError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS controller_state (id INTEGER PRIMARY KEY CHECK (id = 1), draining INTEGER NOT NULL DEFAULT 0 CHECK (draining IN (0, 1)), drain_requested_at_ms INTEGER)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "INSERT OR IGNORE INTO controller_state (id, draining, drain_requested_at_ms) VALUES (1, 0, NULL)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    validation::validate_intent_schema(conn).await?;
    conn.execute("PRAGMA user_version = 2", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn migrate_version_two(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = validation::read_columns(conn).await?;
    for (column, definition) in [
        ("message_id", "INTEGER"),
        ("runner_request_id", "INTEGER"),
        ("requested_workflow_run_id", "INTEGER"),
        ("requested_job_id", "TEXT"),
        ("runner_name", "TEXT"),
        ("observed_job_id", "TEXT"),
        ("observed_workflow_run_id", "INTEGER"),
        ("remote_terminal", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !columns.contains(column) {
            conn.execute(
                &format!("ALTER TABLE intents ADD COLUMN {column} {definition}"),
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        }
    }
    validation::validate_v3_schema(conn).await?;
    conn.execute("PRAGMA user_version = 3", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn migrate_version_three(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = validation::read_columns(conn).await?;
    if !columns.contains("replay_key_version") {
        conn.execute(
            "ALTER TABLE intents ADD COLUMN replay_key_version INTEGER NOT NULL DEFAULT 0 CHECK (replay_key_version IN (0, 1))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    validation::validate_v4_schema(conn).await?;
    conn.execute("PRAGMA user_version = 4", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn migrate_version_four(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = validation::read_columns(conn).await?;
    if !columns.contains("effect_state") {
        conn.execute(
            "ALTER TABLE intents ADD COLUMN effect_state TEXT NOT NULL DEFAULT 'unknown' CHECK (effect_state IN ('unknown', 'not_started', 'may_have_effect', 'definite_no_effect'))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    validation::validate_v5_schema(conn).await?;
    conn.execute("PRAGMA user_version = 5", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn migrate_version_five(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = validation::read_columns(conn).await?;
    for (column, definition) in [
        ("outer_network_name", "TEXT"),
        ("outer_network_id", "TEXT"),
        (
            "runner_start_state",
            "TEXT NOT NULL DEFAULT 'unknown_legacy' CHECK (runner_start_state IN ('unknown_legacy', 'not_requested', 'may_have_started'))",
        ),
    ] {
        if !columns.contains(column) {
            conn.execute(
                &format!("ALTER TABLE intents ADD COLUMN {column} {definition}"),
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        }
    }
    validation::validate_v6_schema(conn).await?;
    conn.execute("PRAGMA user_version = 6", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn migrate_version_six(conn: &turso::Connection) -> Result<(), HostError> {
    validation::validate_v6_schema(conn).await?;
    // The pre-v7 marker recorded no exact resource, diagnostics, or ownership
    // evidence. It cannot survive as proof that a launch reservation is free.
    conn.execute(
        "UPDATE intents SET cleanup_proven = 0 WHERE kind = 'launch'",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS worker_cleanup (launch_id INTEGER PRIMARY KEY CHECK (launch_id > 0), post_action_disposition TEXT NOT NULL CHECK (post_action_disposition IN ('completed', 'not_run', 'interrupted', 'unknown')), post_action_reason_class TEXT, stop_policy TEXT NOT NULL CHECK (stop_policy IN ('require_stopped', 'stop_at_deadline')), stop_grace_seconds INTEGER, stop_reason_class TEXT, children_drained INTEGER NOT NULL DEFAULT 0 CHECK (children_drained IN (0, 1)), diagnostics_recorded INTEGER NOT NULL DEFAULT 0 CHECK (diagnostics_recorded IN (0, 1)), diagnostics_relative_path TEXT, diagnostics_sha256 TEXT, diagnostics_bytes INTEGER, diagnostics_redacted INTEGER, diagnostics_retained INTEGER, diagnostics_source_absent INTEGER, complete INTEGER NOT NULL DEFAULT 0 CHECK (complete IN (0, 1)), outer_network_name TEXT, outer_network_id TEXT, runner_start_observation TEXT CHECK (runner_start_observation IS NULL OR runner_start_observation IN ('never_started', 'may_have_started')), cleanup_disposition TEXT, cleanup_reason_class TEXT, cleanup_resources TEXT, observed_attempt INTEGER, observed_actions_job_id INTEGER, observed_runner_name TEXT)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS worker_cleanup_steps (launch_id INTEGER NOT NULL CHECK (launch_id > 0), step_key TEXT NOT NULL, completed INTEGER NOT NULL CHECK (completed IN (0, 1)), PRIMARY KEY (launch_id, step_key))",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS worker_cleanup_resources (launch_id INTEGER NOT NULL CHECK (launch_id > 0), resource_kind TEXT NOT NULL CHECK (resource_kind IN ('container', 'network')), resource_id TEXT NOT NULL, PRIMARY KEY (launch_id, resource_kind, resource_id))",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    validation::validate_cleanup_schema(conn).await?;
    conn.execute("PRAGMA user_version = 7", ())
        .await
        .map_err(|_| HostError::Journal)?;
    migrate_version_seven(conn).await?;
    migrate_version_eight(conn).await
}

async fn migrate_version_seven(conn: &turso::Connection) -> Result<(), HostError> {
    validation::validate_v7_schema(conn).await?;
    let columns = validation::read_columns(conn).await?;
    for (column, definition) in [
        ("observed_actions_attempt", "INTEGER"),
        ("observed_actions_job_id", "INTEGER"),
        ("observed_actions_conclusion", "TEXT"),
    ] {
        if !columns.contains(column) {
            conn.execute(
                &format!("ALTER TABLE intents ADD COLUMN {column} {definition}"),
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        }
    }
    validation::validate_v8_schema(conn).await?;
    conn.execute("PRAGMA user_version = 8", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn migrate_version_eight(conn: &turso::Connection) -> Result<(), HostError> {
    validation::validate_v8_schema(conn).await?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS scale_set_sessions (intent_id INTEGER PRIMARY KEY CHECK (intent_id > 0), session_id TEXT UNIQUE, state TEXT NOT NULL CHECK (state IN ('creating', 'open', 'closed'))) ",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    validation::validate_v9_session_schema(conn).await?;
    conn.execute("PRAGMA user_version = 9", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn migrate_version_nine(conn: &turso::Connection) -> Result<(), HostError> {
    validation::validate_v8_schema(conn).await?;
    validation::validate_v9_session_schema(conn).await?;
    let columns = validation::read_session_columns(conn).await?;
    if !columns.contains("target_repository_full_name") {
        conn.execute(
            "ALTER TABLE scale_set_sessions ADD COLUMN target_repository_full_name TEXT",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    if !columns.contains("close_attempted") {
        conn.execute(
            "ALTER TABLE scale_set_sessions ADD COLUMN close_attempted INTEGER NOT NULL DEFAULT 0 CHECK (close_attempted IN (0, 1))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    validation::validate_session_schema(conn).await?;
    conn.execute("PRAGMA user_version = 10", ())
        .await
        .map_err(|_| HostError::Journal)?;
    validation::validate_v10_schema(conn).await
}

async fn migrate_version_ten(conn: &turso::Connection) -> Result<(), HostError> {
    validation::validate_v10_schema(conn).await?;
    let create_table =
        validation::POPULATION_TABLE_SQL.replacen("CREATE TABLE", "CREATE TABLE IF NOT EXISTS", 1);
    conn.execute(&create_table, ())
        .await
        .map_err(|_| HostError::Journal)?;
    validation::validate_population_schema(conn).await?;
    conn.execute("PRAGMA user_version = 11", ())
        .await
        .map_err(|_| HostError::Journal)?;
    validation::validate_current_schema(conn).await
}

async fn ensure_legacy_columns(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = validation::read_columns(conn).await?;
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
