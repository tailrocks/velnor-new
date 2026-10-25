//! Read one exact workflow run attempt.

use super::{WorkflowRun, WorkflowRunError, get_json, repository_path};
use crate::session::Transport;

/// Get one exact workflow run attempt from GitHub.
///
/// # Errors
///
/// Gives [`WorkflowRunError`] when the request or returned run identity is invalid.
pub fn get_workflow_run_attempt<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    workflow_run_id: i64,
    attempt_number: i64,
    token: &str,
) -> Result<WorkflowRun, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    if workflow_run_id <= 0 || attempt_number <= 0 {
        return Err(WorkflowRunError::InvalidRequest);
    }
    let tail = format!("actions/runs/{workflow_run_id}/attempts/{attempt_number}");
    let path = repository_path(owner, repository, &tail)?;
    let run: WorkflowRun = get_json(transport, path, token)?;
    if run.id != workflow_run_id || run.run_attempt != attempt_number {
        return Err(WorkflowRunError::Malformed);
    }
    Ok(run)
}
