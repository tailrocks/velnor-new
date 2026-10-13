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

pub(super) async fn validate_current_schema(conn: &turso::Connection) -> Result<(), HostError> {
    validate_v6_schema(conn).await?;
    validate_cleanup_schema(conn).await
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
