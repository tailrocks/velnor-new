// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::CompleteSourceMetadata;

use super::super::workflow_ref::workflow_source;
use super::super::{RepositoryTarget, VerifiedJobWorkflowRef, WorkflowRefKind, WorkflowRun};
use super::{same_name, valid_commit_sha};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedWorkflowSource {
    pub repository_id: i64,

    pub owner_name: String,

    pub repository_name: String,

    pub workflow_run_id: i64,

    pub event_name: String,

    pub head_branch: Option<String>,

    pub head_sha: String,

    pub job_workflow_ref: String,

    pub job_workflow: VerifiedJobWorkflowRef,

    pub workflow_source_sha: Option<String>,

    pub run_attempt: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SourceMismatch {
    #[error("workflow source metadata is incomplete")]
    MissingValue,

    #[error("workflow run identifier is different")]
    WorkflowRunId,

    #[error("repository identifier is different")]
    RepositoryId,

    #[error("repository name is different")]
    RepositoryName,

    #[error("workflow event is different")]
    Event,

    #[error("workflow ref is different")]
    WorkflowRef,

    #[error("workflow run commit SHA is invalid")]
    WorkflowSha,
}

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
