//! Correlate actual runner lifecycle events with durable launch generations.

use crate::error::HostError;
use velnor_runner_github::{InnerJob, InnerKind};

use super::{Journal, one_row};

impl Journal {
    /// Persist runner lifecycle events from the official Scale Set queue.
    /// Returns whether an event matched a Velnor launch generation.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for malformed or conflicting event
    /// identity or when the journal cannot commit the observation.
    pub async fn observe_runner_event(&self, event: &InnerJob) -> Result<bool, HostError> {
        self.observe_runner_event_with_id(event)
            .await
            .map(|launch_id| launch_id.is_some())
    }

    /// Persist a lifecycle event and return the exact matched launch generation.
    ///
    /// This is for callers that need to attach read-only follow-up evidence to
    /// the generation matched by this same observation. The identity is still
    /// derived only from the actual `Started` or `Completed` event.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for malformed or conflicting event
    /// identity or when the journal cannot commit the observation.
    pub async fn observe_runner_event_with_id(
        &self,
        event: &InnerJob,
    ) -> Result<Option<i64>, HostError> {
        let Some(identity) = RunnerEventIdentity::from_event(event)? else {
            return Ok(None);
        };
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = persist_runner_event(&conn, identity).await;
        let end = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        end.map_err(|_| HostError::Journal)?;
        result
    }
}

struct RunnerEventIdentity<'a> {
    name: &'a str,
    runner_id: String,
    job_id: &'a str,
    workflow_run_id: i64,
    completed: bool,
}

impl<'a> RunnerEventIdentity<'a> {
    fn from_event(event: &'a InnerJob) -> Result<Option<Self>, HostError> {
        let completed = match &event.kind {
            InnerKind::Started => false,
            InnerKind::Completed => true,
            InnerKind::Available | InnerKind::Assigned | InnerKind::Unsupported(_) => {
                return Ok(None);
            }
        };
        let (Some(name), Some(runner_id)) = (event.runner_name.as_deref(), event.runner_id) else {
            return Ok(None);
        };
        let (Some(job_id), Some(workflow_run_id)) =
            (event.job_id.as_deref(), event.workflow_run_id)
        else {
            return Err(HostError::Journal);
        };
        if runner_id <= 0
            || name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || job_id.is_empty()
            || job_id.len() > 256
            || job_id.chars().any(char::is_control)
            || workflow_run_id <= 0
        {
            return Err(HostError::Journal);
        }
        Ok(Some(Self {
            name,
            runner_id: runner_id.to_string(),
            job_id,
            workflow_run_id,
            completed,
        }))
    }
}

async fn persist_runner_event(
    conn: &turso::Connection,
    event: RunnerEventIdentity<'_>,
) -> Result<Option<i64>, HostError> {
    let mut rows = conn
        .query(
            "SELECT id, github_runner_id, observed_job_id, observed_workflow_run_id, cleanup_proven FROM intents WHERE kind = 'launch' AND runner_name = ?1 ORDER BY id",
            [event.name],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let old_runner: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    let old_job: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
    let old_run: Option<i64> = row.get(3).map_err(|_| HostError::Journal)?;
    let cleanup_proven: i64 = row.get(4).map_err(|_| HostError::Journal)?;
    drop(rows);
    let mut rows = conn
        .query(
            "SELECT id FROM intents WHERE kind = 'launch' AND runner_name = ?1 AND id != ?2",
            (event.name, id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let ambiguous = rows.next().await.map_err(|_| HostError::Journal)?.is_some();
    drop(rows);
    if ambiguous {
        return Err(HostError::Journal);
    }
    if cleanup_proven == 1 {
        return Ok(None);
    }
    if old_runner
        .as_deref()
        .is_some_and(|value| value != event.runner_id)
        || old_job
            .as_deref()
            .is_some_and(|value| value != event.job_id)
        || old_run.is_some_and(|value| value != event.workflow_run_id)
    {
        return Err(HostError::Journal);
    }
    let changed = conn
        .execute(
            "UPDATE intents SET github_runner_id = COALESCE(github_runner_id, ?1), observed_job_id = COALESCE(observed_job_id, ?2), observed_workflow_run_id = COALESCE(observed_workflow_run_id, ?3), remote_terminal = CASE WHEN ?4 = 1 THEN 1 ELSE remote_terminal END WHERE id = ?5",
            (
                event.runner_id,
                event.job_id,
                event.workflow_run_id,
                i64::from(event.completed),
                id,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    one_row(changed)?;
    Ok(Some(id))
}
