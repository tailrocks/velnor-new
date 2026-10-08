//! Read and validate persisted cleanup checkpoint state.

use crate::error::HostError;
use crate::journal::{LaunchEffectState, PostActionDisposition};

use super::validation::read_flag;
use super::{
    CleanupCheckpointIdentity, CleanupChildren, CleanupDiagnostics, RunnerStartObservation,
};

pub(super) struct ExistingCleanup {
    pub(super) post_state: String,
    pub(super) post_reason: Option<String>,
    pub(super) policy: String,
    pub(super) grace: Option<i64>,
    pub(super) policy_reason: Option<String>,
    pub(super) outer_network_name: Option<String>,
    pub(super) outer_network_id: Option<String>,
    pub(super) children_drained: bool,
    pub(super) diagnostics_recorded: bool,
    pub(super) diagnostics_relative_path: Option<String>,
    pub(super) diagnostics_sha256: Option<String>,
    pub(super) diagnostics_bytes: Option<i64>,
    pub(super) diagnostics_redacted: Option<i64>,
    pub(super) diagnostics_retained: Option<i64>,
    pub(super) diagnostics_source_absent: Option<i64>,
    pub(super) complete: bool,
    pub(super) runner_start_observation: Option<RunnerStartObservation>,
}

pub(super) async fn cleanup_row(
    conn: &turso::Connection,
    launch_id: i64,
) -> Result<Option<ExistingCleanup>, HostError> {
    let mut rows = conn
        .query(
            "SELECT post_action_disposition, post_action_reason_class, stop_policy, stop_grace_seconds, stop_reason_class, outer_network_name, outer_network_id, children_drained, diagnostics_recorded, diagnostics_relative_path, diagnostics_sha256, diagnostics_bytes, diagnostics_redacted, diagnostics_retained, diagnostics_source_absent, complete, runner_start_observation FROM worker_cleanup WHERE launch_id = ?1",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let start_text = row
        .get::<Option<String>>(16)
        .map_err(|_| HostError::Journal)?;
    Ok(Some(ExistingCleanup {
        post_state: row.get(0).map_err(|_| HostError::Journal)?,
        post_reason: row.get(1).map_err(|_| HostError::Journal)?,
        policy: row.get(2).map_err(|_| HostError::Journal)?,
        grace: row.get(3).map_err(|_| HostError::Journal)?,
        policy_reason: row.get(4).map_err(|_| HostError::Journal)?,
        outer_network_name: row.get(5).map_err(|_| HostError::Journal)?,
        outer_network_id: row.get(6).map_err(|_| HostError::Journal)?,
        children_drained: read_flag(row.get(7).map_err(|_| HostError::Journal)?)?,
        diagnostics_recorded: read_flag(row.get(8).map_err(|_| HostError::Journal)?)?,
        diagnostics_relative_path: row.get(9).map_err(|_| HostError::Journal)?,
        diagnostics_sha256: row.get(10).map_err(|_| HostError::Journal)?,
        diagnostics_bytes: row.get(11).map_err(|_| HostError::Journal)?,
        diagnostics_redacted: row.get(12).map_err(|_| HostError::Journal)?,
        diagnostics_retained: row.get(13).map_err(|_| HostError::Journal)?,
        diagnostics_source_absent: row.get(14).map_err(|_| HostError::Journal)?,
        complete: read_flag(row.get(15).map_err(|_| HostError::Journal)?)?,
        runner_start_observation: start_text
            .as_deref()
            .map(RunnerStartObservation::parse)
            .transpose()?,
    }))
}

pub(super) async fn validate_generation(
    conn: &turso::Connection,
    identity: &CleanupCheckpointIdentity,
    post_actions: &PostActionDisposition,
) -> Result<(), HostError> {
    let generation = generation_row(conn, identity.launch_id).await?;
    if generation_matches(&generation, identity, post_actions) {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

struct GenerationRow {
    kind: String,
    state: String,
    effect: LaunchEffectState,
    cleanup_proven: bool,
    runner_name: Option<String>,
    worker_volume: Option<String>,
    docker_id: Option<String>,
    runner_id: Option<i64>,
    dind_id: Option<String>,
    outer_network_name: Option<String>,
    outer_network_id: Option<String>,
    observed_workflow_run_id: Option<i64>,
    observed_job_id: Option<String>,
    remote_terminal: bool,
    runner_start_state: Option<String>,
}

async fn generation_row(
    conn: &turso::Connection,
    launch_id: i64,
) -> Result<GenerationRow, HostError> {
    let mut rows = conn
        .query(
            "SELECT kind, state, effect_state, cleanup_proven, runner_name, worker_volume, docker_id, dind_id, outer_network_name, outer_network_id, observed_workflow_run_id, observed_job_id, github_runner_id, remote_terminal, runner_start_state FROM intents WHERE id = ?1",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let runner_id = row
        .get::<Option<String>>(12)
        .map_err(|_| HostError::Journal)?
        .map(|value| value.parse::<i64>().map_err(|_| HostError::Journal))
        .transpose()?;
    Ok(GenerationRow {
        kind: row.get(0).map_err(|_| HostError::Journal)?,
        state: row.get(1).map_err(|_| HostError::Journal)?,
        effect: LaunchEffectState::parse(&row.get::<String>(2).map_err(|_| HostError::Journal)?)?,
        cleanup_proven: read_flag(row.get(3).map_err(|_| HostError::Journal)?)?,
        runner_name: row.get(4).map_err(|_| HostError::Journal)?,
        worker_volume: row.get(5).map_err(|_| HostError::Journal)?,
        docker_id: row.get(6).map_err(|_| HostError::Journal)?,
        runner_id,
        dind_id: row.get(7).map_err(|_| HostError::Journal)?,
        outer_network_name: row.get(8).map_err(|_| HostError::Journal)?,
        outer_network_id: row.get(9).map_err(|_| HostError::Journal)?,
        observed_workflow_run_id: row.get(10).map_err(|_| HostError::Journal)?,
        observed_job_id: row.get(11).map_err(|_| HostError::Journal)?,
        remote_terminal: read_flag(row.get(13).map_err(|_| HostError::Journal)?)?,
        runner_start_state: row.get(14).map_err(|_| HostError::Journal)?,
    })
}

fn generation_matches(
    generation: &GenerationRow,
    identity: &CleanupCheckpointIdentity,
    post_actions: &PostActionDisposition,
) -> bool {
    let expected_start = match identity_start_observation(post_actions) {
        RunnerStartObservation::NeverStarted => Some("not_requested"),
        RunnerStartObservation::MayHaveStarted => Some("may_have_started"),
    };
    generation.kind == "launch"
        && generation.state == "done"
        && generation.effect == LaunchEffectState::MayHaveEffect
        && generation.remote_terminal
        && !generation.cleanup_proven
        && generation.runner_name.as_deref() == Some(identity.expected_runner_name.as_str())
        && generation.worker_volume.as_deref() == Some(identity.worker_volume.as_str())
        && generation.docker_id.as_deref() == Some(identity.runner_container_id.as_str())
        && generation.dind_id.as_deref() == Some(identity.dind_container_id.as_str())
        && generation.outer_network_name == identity.outer_network_name
        && generation.outer_network_id == identity.outer_network_id
        && generation.observed_workflow_run_id == identity.observed_workflow_run_id
        && generation.observed_job_id == identity.observed_job_id
        && generation.runner_id == identity.observed_runner_id
        && identity.observed_attempt.is_none()
        && identity.observed_actions_job_id.is_none()
        && identity
            .observed_runner_name
            .as_deref()
            .is_none_or(|name| name == identity.expected_runner_name)
        && generation.runner_start_state.as_deref() == expected_start
}

fn identity_start_observation(post_actions: &PostActionDisposition) -> RunnerStartObservation {
    match post_actions {
        PostActionDisposition::NotRun => RunnerStartObservation::NeverStarted,
        PostActionDisposition::Completed
        | PostActionDisposition::Interrupted { .. }
        | PostActionDisposition::Unknown => RunnerStartObservation::MayHaveStarted,
    }
}

pub(super) async fn ensure_cleanup_open(
    conn: &turso::Connection,
    launch_id: i64,
) -> Result<(), HostError> {
    let cleanup = cleanup_row(conn, launch_id)
        .await?
        .ok_or(HostError::Journal)?;
    let mut rows = conn
        .query(
            "SELECT cleanup_proven FROM intents WHERE id = ?1 AND kind = 'launch'",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let proven = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if launch_id <= 0 || cleanup.complete || read_flag(proven)? {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

pub(super) async fn cleanup_children(
    conn: &turso::Connection,
    launch_id: i64,
) -> Result<CleanupChildren, HostError> {
    let mut rows = conn
        .query(
            "SELECT resource_kind, resource_id FROM worker_cleanup_resources WHERE launch_id = ?1 ORDER BY resource_kind, resource_id",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut children = CleanupChildren::default();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let kind: String = row.get(0).map_err(|_| HostError::Journal)?;
        let id: String = row.get(1).map_err(|_| HostError::Journal)?;
        match kind.as_str() {
            "container" => children.containers.push(id),
            "network" => children.networks.push(id),
            _ => return Err(HostError::Journal),
        }
    }
    Ok(children)
}

pub(super) async fn children_drained(
    conn: &turso::Connection,
    launch_id: i64,
) -> Result<bool, HostError> {
    let mut rows = conn
        .query(
            "SELECT children_drained FROM worker_cleanup WHERE launch_id = ?1",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    read_flag(value)
}

pub(super) async fn diagnostics_match(
    conn: &turso::Connection,
    launch_id: i64,
    receipt: &CleanupDiagnostics,
) -> Result<bool, HostError> {
    let mut rows = conn
        .query(
            "SELECT diagnostics_relative_path, diagnostics_sha256, diagnostics_bytes, diagnostics_redacted, diagnostics_retained, diagnostics_source_absent FROM worker_cleanup WHERE launch_id = ?1 AND diagnostics_recorded = 1",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    Ok(
        row.get::<String>(0).map_err(|_| HostError::Journal)? == receipt.relative_path
            && row.get::<String>(1).map_err(|_| HostError::Journal)? == receipt.sha256
            && row.get::<i64>(2).map_err(|_| HostError::Journal)?
                == i64::try_from(receipt.bytes).map_err(|_| HostError::Journal)?
            && row.get::<i64>(3).map_err(|_| HostError::Journal)? == i64::from(receipt.redacted)
            && row.get::<i64>(4).map_err(|_| HostError::Journal)? == i64::from(receipt.retained)
            && row.get::<i64>(5).map_err(|_| HostError::Journal)?
                == i64::from(receipt.source_absent),
    )
}

pub(super) async fn step_completed(
    conn: &turso::Connection,
    launch_id: i64,
    step_key: &str,
) -> Result<bool, HostError> {
    let mut rows = conn
        .query(
            "SELECT completed FROM worker_cleanup_steps WHERE launch_id = ?1 AND step_key = ?2",
            (launch_id, step_key),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    read_flag(row.get(0).map_err(|_| HostError::Journal)?)
}

pub(super) async fn step_intended(
    conn: &turso::Connection,
    launch_id: i64,
    step_key: &str,
) -> Result<bool, HostError> {
    let mut rows = conn
        .query(
            "SELECT 1 FROM worker_cleanup_steps WHERE launch_id = ?1 AND step_key = ?2",
            (launch_id, step_key),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}
