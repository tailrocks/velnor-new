//! Compare bounded hook source values with GitHub records.

use super::{PullRequest, WorkflowRun, WorkflowRunError, verify_pull_request};

#[path = "hook_source/compare.rs"]
mod compare;
#[path = "hook_source/input.rs"]
mod input;
pub use compare::compare_hook_source;
pub use input::{UntrustedHookPullRequest, UntrustedHookSourceInput};

/// Pull request facts compared with the event, run, and current API record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparedHookPullRequestFacts {
    number: i64,
    base_repository_id: i64,
    base_repository: String,
    base_ref: String,
    base_sha: String,
    head_repository_id: i64,
    head_repository: String,
    head_ref: String,
    head_sha: String,
}

impl ComparedHookPullRequestFacts {
    /// Return the exact pull request number and base and head identities.
    #[must_use]
    pub fn identity(&self) -> (i64, i64, &str, &str, i64, &str, &str, &str) {
        (
            self.number,
            self.base_repository_id,
            &self.base_repository,
            &self.base_ref,
            self.head_repository_id,
            &self.head_repository,
            &self.head_ref,
            &self.head_sha,
        )
    }

    /// Return the exact base and head commit SHAs.
    #[must_use]
    pub fn commit_shas(&self) -> (&str, &str) {
        (&self.base_sha, &self.head_sha)
    }
}

/// Source facts compared with an exact run attempt and current source records.
///
/// This record does not authorize a host operation or create a permit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparedHookSourceFacts {
    repository_id: i64,
    repository: String,
    run_id: i64,
    run_attempt: i64,
    event_name: String,
    event_ref: String,
    tested_sha: String,
    workflow_ref: String,
    workflow_sha: String,
    job_workflow_ref: String,
    workflow_job_id: String,
    job_workflow_sha: Option<String>,
    pull_request: Option<ComparedHookPullRequestFacts>,
}

impl ComparedHookSourceFacts {
    /// Return the repository ID that matched the configured scope.
    #[must_use]
    pub const fn repository_id(&self) -> i64 {
        self.repository_id
    }

    /// Return the exact workflow run ID and attempt.
    #[must_use]
    pub const fn run_identity(&self) -> (i64, i64) {
        (self.run_id, self.run_attempt)
    }

    /// Return the compared event name, ref, and tested commit SHA.
    #[must_use]
    pub fn event_source(&self) -> (&str, &str, &str) {
        (&self.event_name, &self.event_ref, &self.tested_sha)
    }

    /// Return the root workflow ref and exact workflow commit SHA.
    #[must_use]
    pub fn workflow_source(&self) -> (&str, &str) {
        (&self.workflow_ref, &self.workflow_sha)
    }

    /// Return the matched native job workflow ref and immutable source SHA, when reusable.
    #[must_use]
    pub fn job_workflow(&self) -> (&str, Option<&str>) {
        (&self.job_workflow_ref, self.job_workflow_sha.as_deref())
    }

    /// Return the workflow job key from the runner context.
    #[must_use]
    pub fn workflow_job_id(&self) -> &str {
        &self.workflow_job_id
    }

    /// Return pull request facts for a pull request event.
    #[must_use]
    pub const fn pull_request(&self) -> Option<&ComparedHookPullRequestFacts> {
        self.pull_request.as_ref()
    }

    /// Return the canonical repository name that matched the configured scope.
    #[must_use]
    pub fn repository(&self) -> &str {
        &self.repository
    }
}

/// Source comparison failure. The values do not represent host authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HookSourceError {
    /// The hook input is too large.
    #[error("hook source input is too large")]
    TooLarge,
    /// The hook input is malformed or outside its bounds.
    #[error("hook source input is malformed")]
    InvalidInput,
    /// GitHub records do not match the hook input.
    #[error("hook source does not match GitHub records")]
    Mismatch,
    /// A GitHub request failed or returned uncertain data.
    #[error(transparent)]
    Api(#[from] WorkflowRunError),
}

fn compare_pull(
    input: &input::UntrustedHookSourceInput,
    run: &WorkflowRun,
    pull: &PullRequest,
    repository_id: i64,
) -> Result<ComparedHookPullRequestFacts, HookSourceError> {
    let event = input
        .pull_request
        .as_ref()
        .ok_or(HookSourceError::Mismatch)?;
    if pull.state != "open" || pull.merged || pull.number != event.number {
        return Err(HookSourceError::Mismatch);
    }
    let verified =
        verify_pull_request(run, pull, repository_id).map_err(|_| HookSourceError::Mismatch)?;
    let base = pull
        .base
        .repository
        .as_ref()
        .ok_or(HookSourceError::Mismatch)?;
    let head = pull
        .head
        .repository
        .as_ref()
        .ok_or(HookSourceError::Mismatch)?;
    let run_pull = run
        .pull_requests
        .iter()
        .find(|run_pull| run_pull.number == event.number)
        .ok_or(HookSourceError::Mismatch)?;
    let run_base = run_pull
        .base
        .repository
        .as_ref()
        .ok_or(HookSourceError::Mismatch)?;
    let run_head = run_pull
        .head
        .repository
        .as_ref()
        .ok_or(HookSourceError::Mismatch)?;
    if verified.base_repository_id != event.base.repository.id
        || verified.head_repository_id != event.head.repository.id
        || run_base.id != event.base.repository.id
        || run_head.id != event.head.repository.id
        || !same_name(&run_base.name, &event.base.repository.name)
        || !same_name(&run_head.name, &event.head.repository.name)
        || !same_name(&base.name, &event.base.repository.name)
        || !same_name(&base.full_name, &event.base.repository.full_name)
        || !same_name(&head.name, &event.head.repository.name)
        || !same_name(&head.full_name, &event.head.repository.full_name)
        || verified.base_ref != event.base.reference
        || verified.base_sha != event.base.sha
        || verified.head_ref != event.head.reference
        || verified.head_sha != event.head.sha
    {
        return Err(HookSourceError::Mismatch);
    }
    Ok(ComparedHookPullRequestFacts {
        number: event.number,
        base_repository_id: event.base.repository.id,
        base_repository: event.base.repository.full_name.clone(),
        base_ref: event.base.reference.clone(),
        base_sha: event.base.sha.clone(),
        head_repository_id: event.head.repository.id,
        head_repository: event.head.repository.full_name.clone(),
        head_ref: event.head.reference.clone(),
        head_sha: event.head.sha.clone(),
    })
}

fn same_name(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}
