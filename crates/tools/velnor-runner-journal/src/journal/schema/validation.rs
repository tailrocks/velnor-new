//! Validate journal schema versions and table layouts.

use std::collections::HashSet;

use crate::error::HostError;

const BASE_COLUMNS: [&str; 9] = [
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
const CURRENT_COLUMNS: [&str; 22] = [
    "id",
    "kind",
    "subject",
    "state",
    "docker_id",
    "github_runner_id",
    "cleanup_proven",
    "dind_id",
    "worker_volume",
    "message_id",
    "runner_request_id",
    "requested_workflow_run_id",
    "requested_job_id",
    "runner_name",
    "observed_job_id",
    "observed_workflow_run_id",
    "remote_terminal",
    "replay_key_version",
    "effect_state",
    "outer_network_name",
    "outer_network_id",
    "runner_start_state",
];
const ACTIONS_RECONCILIATION_COLUMNS: [&str; 3] = [
    "observed_actions_attempt",
    "observed_actions_job_id",
    "observed_actions_conclusion",
];

pub(super) const POPULATION_TABLE_SQL: &str = "CREATE TABLE scale_set_population_observations (intent_id INTEGER PRIMARY KEY CHECK (intent_id > 0), session_id TEXT NOT NULL, scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), source TEXT NOT NULL CHECK (source IN ('session_created', 'poll_batch')), message_id INTEGER, observed_at_ms INTEGER NOT NULL CHECK (observed_at_ms >= 0), total_available_jobs INTEGER NOT NULL, total_acquired_jobs INTEGER NOT NULL, total_assigned_jobs INTEGER NOT NULL, total_running_jobs INTEGER NOT NULL, total_registered_runners INTEGER NOT NULL, total_busy_runners INTEGER NOT NULL, total_idle_runners INTEGER NOT NULL, CHECK ((source = 'session_created' AND message_id IS NULL) OR (source = 'poll_batch' AND message_id IS NOT NULL AND message_id >= 0)))";

pub(super) async fn validate_v6_schema(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_columns(conn).await?;
    if !CURRENT_COLUMNS
        .iter()
        .all(|column| columns.contains(*column))
    {
        return Err(HostError::Journal);
    }
    validate_controller_schema(conn).await
}

pub(super) async fn validate_v7_schema(conn: &turso::Connection) -> Result<(), HostError> {
    validate_v6_schema(conn).await?;
    validate_cleanup_schema(conn).await
}

pub(super) async fn validate_v8_schema(conn: &turso::Connection) -> Result<(), HostError> {
    validate_v7_schema(conn).await?;
    let columns = read_columns(conn).await?;
    if ACTIONS_RECONCILIATION_COLUMNS
        .iter()
        .all(|column| columns.contains(*column))
    {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn validate_current_schema(conn: &turso::Connection) -> Result<(), HostError> {
    validate_v10_schema(conn).await?;
    validate_population_schema(conn).await
}

pub(super) async fn validate_v10_schema(conn: &turso::Connection) -> Result<(), HostError> {
    validate_v8_schema(conn).await?;
    validate_session_schema(conn).await
}

pub(super) async fn validate_population_schema(conn: &turso::Connection) -> Result<(), HostError> {
    validate_population_columns(conn).await?;
    validate_population_definition(conn).await?;
    validate_population_rows(conn).await
}

async fn validate_population_columns(conn: &turso::Connection) -> Result<(), HostError> {
    const COLUMNS: [(&str, &str, i64, i64); 13] = [
        ("intent_id", "INTEGER", 0, 1),
        ("session_id", "TEXT", 1, 0),
        ("scale_set_id", "INTEGER", 1, 0),
        ("source", "TEXT", 1, 0),
        ("message_id", "INTEGER", 0, 0),
        ("observed_at_ms", "INTEGER", 1, 0),
        ("total_available_jobs", "INTEGER", 1, 0),
        ("total_acquired_jobs", "INTEGER", 1, 0),
        ("total_assigned_jobs", "INTEGER", 1, 0),
        ("total_running_jobs", "INTEGER", 1, 0),
        ("total_registered_runners", "INTEGER", 1, 0),
        ("total_busy_runners", "INTEGER", 1, 0),
        ("total_idle_runners", "INTEGER", 1, 0),
    ];
    let mut rows = conn
        .query("PRAGMA table_info(scale_set_population_observations)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    for (expected_name, expected_type, expected_not_null, expected_primary_key) in COLUMNS {
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        if row.get::<String>(1).map_err(|_| HostError::Journal)? != expected_name
            || row.get::<String>(2).map_err(|_| HostError::Journal)? != expected_type
            || row.get::<i64>(3).map_err(|_| HostError::Journal)? != expected_not_null
            || row.get::<i64>(5).map_err(|_| HostError::Journal)? != expected_primary_key
        {
            return Err(HostError::Journal);
        }
    }
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_population_definition(conn: &turso::Connection) -> Result<(), HostError> {
    let mut definition = conn
        .query(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'scale_set_population_observations'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let actual_sql = definition
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    if normalize_schema_sql(&actual_sql) != normalize_schema_sql(POPULATION_TABLE_SQL)
        || definition
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .is_some()
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_population_rows(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM scale_set_population_observations AS p LEFT JOIN scale_set_sessions AS s ON s.intent_id = p.intent_id WHERE s.intent_id IS NULL OR s.session_id IS NULL OR s.session_id != p.session_id OR p.scale_set_id <= 0 OR p.observed_at_ms < 0 OR p.source NOT IN ('session_created', 'poll_batch') OR (p.source = 'session_created' AND p.message_id IS NOT NULL) OR (p.source = 'poll_batch' AND (p.message_id IS NULL OR p.message_id < 0))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let invalid = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if invalid == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn normalize_schema_sql(sql: &str) -> String {
    sql.chars()
        .filter(|character| !character.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) async fn validate_v9_session_schema(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_session_columns(conn).await?;
    if ["intent_id", "session_id", "state"]
        .iter()
        .all(|column| columns.contains(*column))
    {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn validate_session_schema(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_session_columns(conn).await?;
    if ![
        "intent_id",
        "session_id",
        "state",
        "target_repository_full_name",
        "close_attempted",
    ]
    .iter()
    .all(|column| columns.contains(*column))
    {
        return Err(HostError::Journal);
    }
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM scale_set_sessions WHERE close_attempted NOT IN (0, 1) OR (close_attempted = 1 AND session_id IS NULL)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let invalid = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if invalid == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn read_session_columns(
    conn: &turso::Connection,
) -> Result<HashSet<String>, HostError> {
    let mut columns = HashSet::new();
    let mut rows = conn
        .query("PRAGMA table_info(scale_set_sessions)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        columns.insert(row.get::<String>(1).map_err(|_| HostError::Journal)?);
    }
    Ok(columns)
}

pub(super) async fn validate_cleanup_schema(conn: &turso::Connection) -> Result<(), HostError> {
    for (table, expected) in [
        (
            "worker_cleanup",
            &[
                "launch_id",
                "post_action_disposition",
                "post_action_reason_class",
                "stop_policy",
                "stop_grace_seconds",
                "stop_reason_class",
                "children_drained",
                "diagnostics_recorded",
                "diagnostics_relative_path",
                "diagnostics_sha256",
                "diagnostics_bytes",
                "diagnostics_redacted",
                "diagnostics_retained",
                "diagnostics_source_absent",
                "complete",
                "outer_network_name",
                "outer_network_id",
                "runner_start_observation",
                "cleanup_disposition",
                "cleanup_reason_class",
                "cleanup_resources",
                "observed_attempt",
                "observed_actions_job_id",
                "observed_runner_name",
            ][..],
        ),
        (
            "worker_cleanup_steps",
            &["launch_id", "step_key", "completed"][..],
        ),
        (
            "worker_cleanup_resources",
            &["launch_id", "resource_kind", "resource_id"][..],
        ),
    ] {
        let mut columns = HashSet::new();
        let mut rows = conn
            .query(&format!("PRAGMA table_info({table})"), ())
            .await
            .map_err(|_| HostError::Journal)?;
        while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
            columns.insert(row.get::<String>(1).map_err(|_| HostError::Journal)?);
        }
        if !expected.iter().all(|column| columns.contains(*column)) {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

pub(super) async fn validate_v4_schema(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_columns(conn).await?;
    if !CURRENT_COLUMNS
        .iter()
        .take(18)
        .all(|column| columns.contains(*column))
    {
        return Err(HostError::Journal);
    }
    validate_controller_schema(conn).await
}

pub(super) async fn validate_v5_schema(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_columns(conn).await?;
    if !CURRENT_COLUMNS
        .iter()
        .take(19)
        .all(|column| columns.contains(*column))
    {
        return Err(HostError::Journal);
    }
    validate_controller_schema(conn).await
}

pub(super) async fn validate_v3_schema(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_columns(conn).await?;
    if !CURRENT_COLUMNS
        .iter()
        .take(17)
        .all(|column| columns.contains(*column))
    {
        return Err(HostError::Journal);
    }
    validate_controller_schema(conn).await
}

pub(super) async fn validate_intent_schema(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_columns(conn).await?;
    if BASE_COLUMNS.iter().all(|column| columns.contains(*column)) {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn validate_controller_schema(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_controller_columns(conn).await?;
    if ["id", "draining", "drain_requested_at_ms"]
        .iter()
        .all(|column| columns.contains(*column))
    {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn read_columns(conn: &turso::Connection) -> Result<HashSet<String>, HostError> {
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

pub(super) async fn read_controller_columns(
    conn: &turso::Connection,
) -> Result<HashSet<String>, HostError> {
    let mut columns = HashSet::new();
    let mut rows = conn
        .query("PRAGMA table_info(controller_state)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        columns.insert(row.get::<String>(1).map_err(|_| HostError::Journal)?);
    }
    Ok(columns)
}
