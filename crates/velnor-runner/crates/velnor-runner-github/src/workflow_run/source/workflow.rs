//! Workflow source checks.

use crate::CompleteSourceMetadata;

use super::super::workflow_ref::workflow_source;
use super::super::{RepositoryTarget, VerifiedJobWorkflowRef, WorkflowRefKind, WorkflowRun};
use super::{same_name, valid_commit_sha};

/// Source values verified against a workflow run from the GitHub API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedWorkflowSource {
    /// Stable repository identifier.
    pub repository_id: i64,
    /// Account name from the API repository.
    pub owner_name: String,
    /// Repository name from the API repository.
    pub repository_name: String,
    /// Workflow run identifier.
    pub workflow_run_id: i64,
    /// Event that started the workflow run.
    pub event_name: String,
    /// Workflow run head branch, when GitHub returns one.
    pub head_branch: Option<String>,
    /// Workflow run head commit.
    pub head_sha: String,
    /// Workflow ref from the authenticated job message.
    pub job_workflow_ref: String,
    /// Typed workflow ref matched with the API response.
    pub job_workflow: VerifiedJobWorkflowRef,
    /// Source commit for a called workflow, when the API identifies it.
    ///
    /// The root workflow source commit is not available for every event.
    pub workflow_source_sha: Option<String>,
    /// Workflow run attempt number.
    pub run_attempt: i64,
}

/// Workflow source values that are different from the GitHub API values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SourceMismatch {
    /// A required source value is absent or empty.
    #[error("workflow source metadata is incomplete")]
    MissingValue,
    /// The workflow run identifier is different.
    #[error("workflow run identifier is different")]
    WorkflowRunId,
    /// The repository identifier is different.
    #[error("repository identifier is different")]
    RepositoryId,
    /// The repository owner or name is different.
    #[error("repository name is different")]
    RepositoryName,
    /// The event is different.
    #[error("workflow event is different")]
    Event,
    /// The workflow path, ref, or source commit is different.
    #[error("workflow ref is different")]
    WorkflowRef,
    /// The workflow run record has an invalid source commit.
    #[error("workflow run commit SHA is invalid")]
    WorkflowSha,
}

/// Compare message values with a workflow run and repository identity.
///
/// # Errors
///
/// Gives [`SourceMismatch`] when a value is absent or different from the API value.
pub fn verify_workflow_source(
    source: CompleteSourceMetadata<'_>,
    run: &WorkflowRun,
    expected: RepositoryTarget<'_>,
) -> Result<VerifiedWorkflowSource, SourceMismatch> {
    if expected.id <= 0
        || expected.owner.is_empty()
        || expected.name.is_empty()
        || source.owner_name.is_empty()
        || source.repository_name.is_empty()
        || source.job_workflow_ref.is_empty()
        || source.workflow_run_id <= 0
        || source.event_name.is_empty()
        || run.workflow_id <= 0
        || run.run_attempt <= 0
    {
        return Err(SourceMismatch::MissingValue);
    }
    if run.id != source.workflow_run_id {
        return Err(SourceMismatch::WorkflowRunId);
    }
    if run.repository.id != expected.id {
        return Err(SourceMismatch::RepositoryId);
    }
    if !same_name(&run.repository.owner.login, expected.owner)
        || !same_name(&run.repository.name, expected.name)
        || !same_name(source.owner_name, expected.owner)
        || !same_name(source.repository_name, expected.name)
        || !same_name(
            &run.repository.full_name,
            &format!("{}/{}", expected.owner, expected.name),
        )
    {
        return Err(SourceMismatch::RepositoryName);
    }
    if run.event != source.event_name {
        return Err(SourceMismatch::Event);
    }
    if !valid_commit_sha(&run.head_sha) {
        return Err(SourceMismatch::WorkflowSha);
    }
    let (job_workflow, workflow_source_sha) =
        workflow_source(run, source.job_workflow_ref).ok_or(SourceMismatch::WorkflowRef)?;
    if workflow_source_sha.is_some_and(|sha| !valid_commit_sha(sha)) {
        return Err(SourceMismatch::WorkflowRef);
    }
    if workflow_source_sha.is_none() && !workflow_ref_matches_run(run, &job_workflow) {
        return Err(SourceMismatch::WorkflowRef);
    }
    Ok(VerifiedWorkflowSource {
        repository_id: run.repository.id,
        owner_name: run.repository.owner.login.clone(),
        repository_name: run.repository.name.clone(),
        workflow_run_id: run.id,
        event_name: run.event.clone(),
        head_branch: run.head_branch.clone(),
        head_sha: run.head_sha.clone(),
        job_workflow_ref: source.job_workflow_ref.to_owned(),
        job_workflow,
        workflow_source_sha: workflow_source_sha.map(str::to_owned),
        run_attempt: run.run_attempt,
    })
}

fn workflow_ref_matches_run(run: &WorkflowRun, workflow: &VerifiedJobWorkflowRef) -> bool {
    match workflow.kind {
        WorkflowRefKind::Branch => {
            workflow
                .reference
                .strip_prefix("refs/heads/")
                .is_some_and(|branch| {
                    run.event != "pull_request" && run.head_branch.as_deref() == Some(branch)
                })
        }
        WorkflowRefKind::Tag => {
            matches!(run.event.as_str(), "push" | "release" | "workflow_dispatch")
                && workflow
                    .reference
                    .strip_prefix("refs/tags/")
                    .is_some_and(|tag| run.head_branch.as_deref() == Some(tag))
        }
        WorkflowRefKind::PullRequest => {
            run.event == "pull_request" && pull_request_ref_matches(run, &workflow.reference)
        }
        WorkflowRefKind::MergeQueue => run.event == "merge_group",
        WorkflowRefKind::Commit => workflow.reference == run.head_sha,
        WorkflowRefKind::Other => false,
    }
}

fn pull_request_ref_matches(run: &WorkflowRun, reference: &str) -> bool {
    let Some(number) = reference
        .strip_prefix("refs/pull/")
        .and_then(|pull_ref| pull_ref.strip_suffix("/merge"))
        .and_then(|number| number.parse::<i64>().ok())
        .filter(|number| *number > 0)
    else {
        return false;
    };
    let mut pull_requests = run
        .pull_requests
        .iter()
        .filter(|pull_request| pull_request.number == number);
    let Some(pull_request) = pull_requests.next() else {
        return false;
    };
    if pull_requests.next().is_some() {
        return false;
    }
    let Some(head_repository) = run.head_repository.as_ref() else {
        return false;
    };
    if head_repository.id <= 0 {
        return false;
    }
    let Some(run_head_repository) = pull_request.head.repository.as_ref() else {
        return false;
    };
    let Some(run_base_repository) = pull_request.base.repository.as_ref() else {
        return false;
    };
    run_head_repository.id > 0
        && !run_head_repository.name.is_empty()
        && run_head_repository.id == head_repository.id
        && same_name(&run_head_repository.name, &head_repository.name)
        && run_base_repository.id == run.repository.id
        && !run_base_repository.name.is_empty()
        && same_name(&run_base_repository.name, &run.repository.name)
}
