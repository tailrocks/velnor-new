//! Empty-only retry policy for exact-source workflow-run inventory reads.

use super::WorkflowRunResponse;
use super::{RunSource, WorkflowMetadata, run_matches, run_repository_names_match};
use crate::{OrchestratorError, internal::internal};
use std::time::Duration;
use velnor_actions_mise::CheckDeadline;

pub(super) const WORKFLOW_RUN_PAGE_SIZE: usize = 100;
pub(super) const MAX_WORKFLOW_RUN_PAGES: usize = 10;
pub(super) const EMPTY_WORKFLOW_RUN_ATTEMPTS: usize = 6;
const EMPTY_WORKFLOW_RUN_BACKOFFS: [Duration; 5] = [
    Duration::from_secs(3),
    Duration::from_secs(6),
    Duration::from_secs(12),
    Duration::from_secs(24),
    Duration::from_secs(45),
];

pub(super) fn latest_ci_run(
    runs: Vec<WorkflowRunResponse>,
    workflow: &WorkflowMetadata,
    source: RunSource<'_>,
    repository: &str,
) -> Result<WorkflowRunResponse, OrchestratorError> {
    // The head-SHA-only API query also returns eligible manual dispatches. As
    // before the retry was added, only push runs can satisfy the CI gate.
    let runs: Vec<_> = runs
        .into_iter()
        .filter(|run| run.event.as_deref() == Some("push"))
        .collect();
    if runs.is_empty() {
        return Err(internal("package_ci_run_missing"));
    }
    if runs.iter().any(|run| {
        run.run_number.is_none_or(|n| n == 0)
            || !run_repository_names_match(run, repository)
            || !run_matches(
                run,
                workflow,
                run.id.unwrap_or(0),
                run.run_attempt.unwrap_or(0),
                source,
                "push",
            )
            || run.id.is_none_or(|id| id == 0)
            || run.run_attempt.is_none_or(|attempt| attempt == 0)
    }) {
        return Err(internal("package_ci_run_inventory_invalid"));
    }
    let mut ordered = runs;
    ordered.sort_by_key(|run| (run.run_number, run.id));
    let ids: std::collections::BTreeSet<_> = ordered.iter().map(|run| run.id).collect();
    if ids.len() != ordered.len()
        || ordered
            .windows(2)
            .any(|pair| pair[0].run_number == pair[1].run_number)
    {
        return Err(internal("package_ci_run_inventory_ambiguous"));
    }
    ordered
        .pop()
        .ok_or_else(|| internal("package_ci_run_missing"))
}

pub(super) fn retry_empty_workflow_run_inventory(
    mut read_page: impl FnMut(usize) -> Result<Vec<WorkflowRunResponse>, OrchestratorError>,
    mut sleep: impl FnMut(Duration) -> Result<(), OrchestratorError>,
    mut check_deadline: impl FnMut() -> Result<(), OrchestratorError>,
) -> Result<Option<Vec<WorkflowRunResponse>>, OrchestratorError> {
    for attempt in 0..EMPTY_WORKFLOW_RUN_ATTEMPTS {
        check_deadline()?;
        let mut runs = Vec::new();
        let mut complete = false;
        for page in 1..=MAX_WORKFLOW_RUN_PAGES {
            check_deadline()?;
            let page_runs = read_page(page)?;
            let count = page_runs.len();
            runs.extend(page_runs);
            check_deadline()?;
            if count < WORKFLOW_RUN_PAGE_SIZE {
                complete = true;
                break;
            }
        }
        if !complete {
            return Err(internal("package_ci_run_pages_exceeded"));
        }
        if !runs.is_empty() {
            return Ok(Some(runs));
        }
        if let Some(backoff) = EMPTY_WORKFLOW_RUN_BACKOFFS.get(attempt) {
            check_deadline()?;
            sleep(*backoff)?;
            check_deadline()?;
        }
    }
    Ok(None)
}

pub(crate) fn sleep_for_retry(delay: Duration) -> Result<(), OrchestratorError> {
    check_retry_deadline()?;
    let clipped = CheckDeadline::after(delay)
        .and_then(CheckDeadline::remaining)
        .map_err(|_| internal("package_ci_run_deadline"))?;
    std::thread::sleep(clipped);
    check_retry_deadline()
}

pub(super) fn check_retry_deadline() -> Result<(), OrchestratorError> {
    CheckDeadline::check_scope().map_err(|_| internal("package_ci_run_deadline"))
}
