//! Persist exact cleanup receipts and reject row/effect mismatches.

use crate::error::HostError;

use super::super::{LaunchEffectState, one_row};
use super::CleanupRecord;
use crate::journal::Journal;

pub(super) async fn persist_cleanup(
    conn: &turso::Connection,
    proof: &CleanupRecord,
) -> Result<(), HostError> {
    let row = cleanup_row(conn, proof.launch_id).await?;
    if row.kind != "launch"
        || row.runner_name.as_deref() != Some(proof.expected_runner_name.as_str())
        || row.worker_volume.as_deref() != Some(proof.worker_volume.as_str())
        || row.runner_id.as_deref() != Some(proof.runner_id.as_str())
        || row.dind_id.as_deref() != Some(proof.dind_id.as_str())
        || row.outer_network_name != proof.outer_network_name
        || row.outer_network_id != proof.outer_network_id
        || row.observed_workflow_run_id != proof.observed_workflow_run_id
        || row.observed_attempt != proof.observed_attempt
        || row.observed_job_id != proof.observed_job_id
        || row.observed_actions_job_id != proof.observed_actions_job_id
        || row.observed_runner_id != proof.observed_runner_id
        || row.observed_runner_name != proof.observed_runner_name
        || row.effect == LaunchEffectState::DefiniteNoEffect
        || !start_observation_matches(
            row.runner_start_state.as_deref(),
            proof.runner_start_observation,
        )
    {
        return Err(HostError::Journal);
    }
    if row.cleanup_proven {
        Journal::validate_cleanup_complete(conn, proof).await?;
        return exact_receipt_matches(conn, proof).await;
    }
    Journal::validate_cleanup_ready(conn, proof).await?;
    store_cleanup(conn, proof).await?;
    Journal::mark_cleanup_complete(conn, proof.launch_id).await
}

fn start_observation_matches(
    state: Option<&str>,
    observation: super::super::RunnerStartObservation,
) -> bool {
    match observation {
        super::super::RunnerStartObservation::NeverStarted => state == Some("not_requested"),
        super::super::RunnerStartObservation::MayHaveStarted => {
            matches!(state, None | Some("may_have_started"))
        }
    }
}

struct CleanupRow {
    kind: String,
    effect: LaunchEffectState,
    runner_name: Option<String>,
    runner_id: Option<String>,
    dind_id: Option<String>,
    worker_volume: Option<String>,
    outer_network_name: Option<String>,
    outer_network_id: Option<String>,
    observed_workflow_run_id: Option<i64>,
    observed_attempt: Option<i64>,
    observed_job_id: Option<String>,
    observed_actions_job_id: Option<i64>,
    observed_runner_id: Option<i64>,
    observed_runner_name: Option<String>,
    cleanup_proven: bool,
    runner_start_state: Option<String>,
}

async fn cleanup_row(conn: &turso::Connection, id: i64) -> Result<CleanupRow, HostError> {
    let mut rows = conn
        .query(
            "SELECT kind, effect_state, runner_name, docker_id, dind_id, worker_volume, outer_network_name, outer_network_id, observed_workflow_run_id, observed_job_id, github_runner_id, cleanup_proven, runner_start_state FROM intents WHERE id = ?1",
            [id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let runner_id = row
        .get::<Option<String>>(10)
        .map_err(|_| HostError::Journal)?
        .map(|value| value.parse::<i64>().map_err(|_| HostError::Journal))
        .transpose()?;
    Ok(CleanupRow {
        kind: row.get(0).map_err(|_| HostError::Journal)?,
        effect: LaunchEffectState::parse(&row.get::<String>(1).map_err(|_| HostError::Journal)?)?,
        runner_name: row.get(2).map_err(|_| HostError::Journal)?,
        runner_id: row.get(3).map_err(|_| HostError::Journal)?,
        dind_id: row.get(4).map_err(|_| HostError::Journal)?,
        worker_volume: row.get(5).map_err(|_| HostError::Journal)?,
        outer_network_name: row.get(6).map_err(|_| HostError::Journal)?,
        outer_network_id: row.get(7).map_err(|_| HostError::Journal)?,
        observed_workflow_run_id: row.get(8).map_err(|_| HostError::Journal)?,
        observed_attempt: None,
        observed_job_id: row.get(9).map_err(|_| HostError::Journal)?,
        observed_actions_job_id: None,
        observed_runner_id: runner_id,
        observed_runner_name: row.get(2).map_err(|_| HostError::Journal)?,
        cleanup_proven: read_flag(row.get(11).map_err(|_| HostError::Journal)?)?,
        runner_start_state: row.get(12).map_err(|_| HostError::Journal)?,
    })
}

async fn store_cleanup(conn: &turso::Connection, proof: &CleanupRecord) -> Result<(), HostError> {
    let changed = conn
        .execute(
            "UPDATE worker_cleanup SET cleanup_disposition = ?1, cleanup_reason_class = ?2, cleanup_resources = ?3 WHERE launch_id = ?4 AND complete = 0 AND children_drained = 1 AND diagnostics_recorded = 1",
            (
                proof.cleanup_state,
                proof.cleanup_reason.clone(),
                proof.cleanup_resources.clone(),
                proof.launch_id,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    one_row(changed)
}

async fn exact_receipt_matches(
    conn: &turso::Connection,
    proof: &CleanupRecord,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT cleanup_disposition, cleanup_reason_class, post_action_disposition, post_action_reason_class, diagnostics_relative_path, diagnostics_sha256, diagnostics_bytes, diagnostics_redacted, diagnostics_source_absent, cleanup_resources FROM worker_cleanup WHERE launch_id = ?1",
            [proof.launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let exact = row.get::<String>(0).map_err(|_| HostError::Journal)? == proof.cleanup_state
        && row
            .get::<Option<String>>(1)
            .map_err(|_| HostError::Journal)?
            == proof.cleanup_reason
        && row.get::<String>(2).map_err(|_| HostError::Journal)? == proof.post_action_state
        && row
            .get::<Option<String>>(3)
            .map_err(|_| HostError::Journal)?
            == proof.post_action_reason
        && row.get::<String>(4).map_err(|_| HostError::Journal)? == proof.diagnostics_path
        && row.get::<String>(5).map_err(|_| HostError::Journal)? == proof.diagnostics_sha256
        && row.get::<i64>(6).map_err(|_| HostError::Journal)? == proof.diagnostics_bytes
        && row.get::<i64>(7).map_err(|_| HostError::Journal)? == 1
        && row.get::<i64>(8).map_err(|_| HostError::Journal)?
            == i64::from(proof.diagnostics_source_absent)
        && row.get::<String>(9).map_err(|_| HostError::Journal)? == proof.cleanup_resources;
    if exact {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn read_flag(value: i64) -> Result<bool, HostError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HostError::Journal),
    }
}
