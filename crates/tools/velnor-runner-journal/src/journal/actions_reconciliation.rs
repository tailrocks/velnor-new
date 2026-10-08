//! Durable CAS for read-only Actions REST reconciliation of an observed runner.

use crate::error::HostError;
use velnor_runner_github::{ActionsJobReconciliation, ActionsJobReconciliationState};

use super::Journal;

struct CompletedEvidence {
    workflow_run_id: i64,
    runner_id: i64,
    runner_name: String,
    scale_set_job_id: String,
    attempt: i64,
    actions_job_id: i64,
    conclusion: Option<String>,
}

struct ExistingCompletion {
    kind: String,
    state: String,
    effect: String,
    cleanup_proven: i64,
    docker_id: Option<String>,
    runner_id: Option<String>,
    runner_name: Option<String>,
    scale_set_job_id: Option<String>,
    workflow_run_id: Option<i64>,
    runner_start_state: String,
    remote_terminal: i64,
    attempt: Option<i64>,
    actions_job_id: Option<i64>,
    conclusion: Option<String>,
    cleanup_started: i64,
}

impl Journal {
    /// Persist a completed REST reconciliation only when it exactly matches
    /// the actual runner identity already stored for this launch generation.
    ///
    /// The REST job ID remains separate from the opaque Scale Set job ID, and
    /// the conclusion is audit data rather than a workload-success decision.
    /// This records remote-terminal evidence but never proves or marks local
    /// physical cleanup. The caller must supply the result of the bounded,
    /// source-repository-checked Actions reconciliation.
    ///
    /// Repeating the exact same result is idempotent. A changed run, runner,
    /// Scale Set ID, REST attempt, REST job ID, or conclusion fails closed.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for a non-completed or malformed result,
    /// a generation mismatch, conflicting prior evidence, or a failed commit.
    pub async fn record_actions_job_reconciliation(
        &self,
        launch_id: i64,
        reconciliation: &ActionsJobReconciliation,
    ) -> Result<(), HostError> {
        let evidence = validate_completion(reconciliation)?;
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = record_completion(&conn, launch_id, &evidence).await;
        let ended = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        ended.map_err(|_| HostError::Journal)?;
        result
    }
}

fn validate_completion(
    reconciliation: &ActionsJobReconciliation,
) -> Result<CompletedEvidence, HostError> {
    if reconciliation.state != ActionsJobReconciliationState::Completed
        || reconciliation.reason.is_some()
    {
        return Err(HostError::Journal);
    }
    let workflow_run_id = reconciliation
        .observed_workflow_run_id
        .filter(|value| *value > 0)
        .ok_or(HostError::Journal)?;
    let runner_id = reconciliation
        .observed_runner_id
        .filter(|value| *value > 0)
        .ok_or(HostError::Journal)?;
    let runner_name = reconciliation
        .observed_runner_name
        .as_deref()
        .filter(|value| valid_runner_name(value))
        .ok_or(HostError::Journal)?
        .to_owned();
    let scale_set_job_id = reconciliation
        .scale_set_job_id
        .as_deref()
        .filter(|value| valid_opaque_job_id(value))
        .ok_or(HostError::Journal)?
        .to_owned();
    let attempt = reconciliation
        .attempt
        .filter(|value| (1..=8).contains(value))
        .map(i64::from)
        .ok_or(HostError::Journal)?;
    let job = reconciliation.job.as_ref().ok_or(HostError::Journal)?;
    let workflow_run = reconciliation
        .workflow_run
        .as_ref()
        .ok_or(HostError::Journal)?;
    if job.id <= 0
        || job.run_id != workflow_run_id
        || job.status != "completed"
        || job.runner_id != Some(runner_id)
        || job.runner_name.as_deref() != Some(runner_name.as_str())
        || workflow_run.id != workflow_run_id
        || !(1..=8).contains(&workflow_run.run_attempt)
        || attempt > workflow_run.run_attempt
        || workflow_run
            .head_repository_full_name
            .as_deref()
            .is_none_or(|name| !valid_repository_name(name))
        || job
            .conclusion
            .as_deref()
            .is_some_and(|value| !valid_conclusion(value))
    {
        return Err(HostError::Journal);
    }
    Ok(CompletedEvidence {
        workflow_run_id,
        runner_id,
        runner_name,
        scale_set_job_id,
        attempt,
        actions_job_id: job.id,
        conclusion: job.conclusion.clone(),
    })
}

async fn record_completion(
    conn: &turso::Connection,
    launch_id: i64,
    evidence: &CompletedEvidence,
) -> Result<(), HostError> {
    let existing = existing_completion(conn, launch_id).await?;
    let expected_runner_id = evidence.runner_id.to_string();
    if existing.kind != "launch"
        || !matches!(existing.state.as_str(), "done" | "uncertain")
        || existing.effect != "may_have_effect"
        || existing.cleanup_proven != 0
        || existing.docker_id.as_deref().is_none_or(str::is_empty)
        || existing.runner_id.as_deref() != Some(expected_runner_id.as_str())
        || existing.runner_name.as_deref() != Some(evidence.runner_name.as_str())
        || existing.scale_set_job_id.as_deref() != Some(evidence.scale_set_job_id.as_str())
        || existing.workflow_run_id != Some(evidence.workflow_run_id)
        || existing.runner_start_state != "may_have_started"
        || !matches!(existing.remote_terminal, 0 | 1)
        || !matches!(existing.cleanup_started, 0 | 1)
    {
        return Err(HostError::Journal);
    }
    if existing.matches(evidence) {
        return Ok(());
    }
    if existing.has_receipt() || existing.cleanup_started != 0 {
        return Err(HostError::Journal);
    }

    let changed = conn
        .execute(
            "UPDATE intents SET observed_actions_attempt = ?1, observed_actions_job_id = ?2, observed_actions_conclusion = ?3, remote_terminal = 1 WHERE id = ?4 AND kind = 'launch' AND state IN ('done', 'uncertain') AND effect_state = 'may_have_effect' AND cleanup_proven = 0 AND docker_id IS NOT NULL AND github_runner_id = ?5 AND runner_name = ?6 AND observed_job_id = ?7 AND observed_workflow_run_id = ?8 AND runner_start_state = 'may_have_started' AND observed_actions_attempt IS NULL AND observed_actions_job_id IS NULL AND observed_actions_conclusion IS NULL AND NOT EXISTS (SELECT 1 FROM worker_cleanup WHERE launch_id = ?4)",
            (
                evidence.attempt,
                evidence.actions_job_id,
                evidence.conclusion.clone(),
                launch_id,
                evidence.runner_id.to_string(),
                evidence.runner_name.clone(),
                evidence.scale_set_job_id.clone(),
                evidence.workflow_run_id,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

impl ExistingCompletion {
    fn has_receipt(&self) -> bool {
        self.attempt.is_some() || self.actions_job_id.is_some() || self.conclusion.is_some()
    }

    fn matches(&self, evidence: &CompletedEvidence) -> bool {
        self.attempt == Some(evidence.attempt)
            && self.actions_job_id == Some(evidence.actions_job_id)
            && self.conclusion == evidence.conclusion
            && self.remote_terminal == 1
    }
}

async fn existing_completion(
    conn: &turso::Connection,
    launch_id: i64,
) -> Result<ExistingCompletion, HostError> {
    let mut rows = conn
        .query(
            "SELECT kind, state, effect_state, cleanup_proven, docker_id, github_runner_id, runner_name, observed_job_id, observed_workflow_run_id, runner_start_state, remote_terminal, observed_actions_attempt, observed_actions_job_id, observed_actions_conclusion, EXISTS (SELECT 1 FROM worker_cleanup WHERE launch_id = ?1) FROM intents WHERE id = ?1",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    Ok(ExistingCompletion {
        kind: row.get(0).map_err(|_| HostError::Journal)?,
        state: row.get(1).map_err(|_| HostError::Journal)?,
        effect: row.get(2).map_err(|_| HostError::Journal)?,
        cleanup_proven: row.get(3).map_err(|_| HostError::Journal)?,
        docker_id: row.get(4).map_err(|_| HostError::Journal)?,
        runner_id: row.get(5).map_err(|_| HostError::Journal)?,
        runner_name: row.get(6).map_err(|_| HostError::Journal)?,
        scale_set_job_id: row.get(7).map_err(|_| HostError::Journal)?,
        workflow_run_id: row.get(8).map_err(|_| HostError::Journal)?,
        runner_start_state: row.get(9).map_err(|_| HostError::Journal)?,
        remote_terminal: row.get(10).map_err(|_| HostError::Journal)?,
        attempt: row.get(11).map_err(|_| HostError::Journal)?,
        actions_job_id: row.get(12).map_err(|_| HostError::Journal)?,
        conclusion: row.get(13).map_err(|_| HostError::Journal)?,
        cleanup_started: row.get(14).map_err(|_| HostError::Journal)?,
    })
}

fn valid_runner_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_opaque_job_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

fn valid_repository_name(value: &str) -> bool {
    let Some((owner, repository)) = value.split_once('/') else {
        return false;
    };
    !owner.is_empty()
        && !repository.is_empty()
        && !repository.contains('/')
        && value.len() <= 201
        && !value.chars().any(char::is_control)
}

fn valid_conclusion(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
