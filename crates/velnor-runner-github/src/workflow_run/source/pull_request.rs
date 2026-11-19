// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{PullRequest, WorkflowRun, WorkflowRunPullRequest};
use super::valid_commit_sha;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPullRequest {
    pub number: i64,

    pub head_repository_id: i64,

    pub base_repository_id: i64,

    pub head_ref: String,

    pub head_sha: String,

    pub base_ref: String,

    pub base_sha: String,

    pub run_head_sha: String,

    pub merge_commit_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPullRequestRunAssociation {
    pub number: i64,

    pub base_repository_id: i64,

    pub head_repository_id: i64,

    pub run_head_sha: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PullRequestMismatch {
    #[error("pull request event or number is different")]
    Event,

    #[error("pull request number is different")]
    Number,

    #[error("pull request head repository is different")]
    HeadRepository,

    #[error("pull request base repository is different")]
    BaseRepository,

    #[error("pull request head is different")]
    Head,

    #[error("pull request base is different")]
    Base,

    #[error("pull request commit SHA is invalid")]
    CommitSha,
}

pub fn verify_pull_request(
    run: &WorkflowRun,
    pull: &PullRequest,
    expected_base_repository_id: i64,
) -> Result<VerifiedPullRequest, PullRequestMismatch> {
    let run_pull = matching_run_pull(run, pull, expected_base_repository_id)?;
    let (head_repository_id, base_repository_id) =
        verified_repository_ids(run, run_pull, pull, expected_base_repository_id)?;
    verify_pull_request_refs(run, run_pull, pull)?;
    let merge_commit_sha = pull.merge_commit_sha.as_deref();
    if merge_commit_sha.is_some_and(|sha| !valid_commit_sha(sha))
        || !valid_commit_sha(&run.head_sha)
        || !valid_commit_sha(&pull.head.sha)
        || !valid_commit_sha(&pull.base.sha)
    {
        return Err(PullRequestMismatch::CommitSha);
    }
    Ok(VerifiedPullRequest {
        number: pull.number,
        head_repository_id,
        base_repository_id,
        head_ref: pull.head.reference.clone(),
        head_sha: pull.head.sha.clone(),
        base_ref: pull.base.reference.clone(),
        base_sha: pull.base.sha.clone(),
        run_head_sha: run.head_sha.clone(),
        merge_commit_sha: merge_commit_sha.map(str::to_owned),
    })
}

pub fn verify_pull_request_run_association(
    run: &WorkflowRun,
    pull_number: i64,
    expected_base_repository_id: i64,
    expected_head_repository_id: i64,
) -> Result<VerifiedPullRequestRunAssociation, PullRequestMismatch> {
    if run.event != "pull_request"
        || pull_number <= 0
        || expected_base_repository_id <= 0
        || expected_head_repository_id <= 0
    {
        return Err(PullRequestMismatch::Event);
    }
    if run.repository.id != expected_base_repository_id {
        return Err(PullRequestMismatch::BaseRepository);
    }
    let mut matching = run
        .pull_requests
        .iter()
        .filter(|pull| pull.number == pull_number);
    let Some(run_pull) = matching.next() else {
        return Err(PullRequestMismatch::Number);
    };
    if matching.next().is_some() {
        return Err(PullRequestMismatch::Number);
    }
    let run_head_repository = run
        .head_repository
        .as_ref()
        .ok_or(PullRequestMismatch::HeadRepository)?;
    let pull_head_repository = run_pull
        .head
        .repository
        .as_ref()
        .ok_or(PullRequestMismatch::HeadRepository)?;
    let pull_base_repository = run_pull
        .base
        .repository
        .as_ref()
        .ok_or(PullRequestMismatch::BaseRepository)?;
    let head_repository_id = run_head_repository.id;
    if head_repository_id <= 0
        || head_repository_id != expected_head_repository_id
        || pull_head_repository.id != head_repository_id
    {
        return Err(PullRequestMismatch::HeadRepository);
    }
    if pull_base_repository.id != expected_base_repository_id {
        return Err(PullRequestMismatch::BaseRepository);
    }
    if !valid_commit_sha(&run.head_sha) {
        return Err(PullRequestMismatch::CommitSha);
    }
    Ok(VerifiedPullRequestRunAssociation {
        number: pull_number,
        base_repository_id: run.repository.id,
        head_repository_id,
        run_head_sha: run.head_sha.clone(),
    })
}

fn matching_run_pull<'a>(
    run: &'a WorkflowRun,
    pull: &PullRequest,
    expected_base_repository_id: i64,
) -> Result<&'a WorkflowRunPullRequest, PullRequestMismatch> {
    if run.event != "pull_request" || expected_base_repository_id <= 0 {
        return Err(PullRequestMismatch::Event);
    }
    if run.repository.id != expected_base_repository_id {
        return Err(PullRequestMismatch::BaseRepository);
    }
    if pull.number <= 0 {
        return Err(PullRequestMismatch::Number);
    }
    run.pull_requests
        .iter()
        .find(|item| item.number == pull.number)
        .ok_or(PullRequestMismatch::Number)
}

fn verified_repository_ids(
    run: &WorkflowRun,
    run_pull: &WorkflowRunPullRequest,
    pull: &PullRequest,
    expected_base_repository_id: i64,
) -> Result<(i64, i64), PullRequestMismatch> {
    let head_id = pull
        .head
        .repository
        .as_ref()
        .map(|repository| repository.id)
        .filter(|id| *id > 0)
        .ok_or(PullRequestMismatch::HeadRepository)?;
    let base_id = pull
        .base
        .repository
        .as_ref()
        .map(|repository| repository.id)
        .filter(|id| *id > 0)
        .ok_or(PullRequestMismatch::BaseRepository)?;
    if base_id != expected_base_repository_id
        || run_pull
            .base
            .repository
            .as_ref()
            .is_some_and(|repository| repository.id != base_id)
    {
        return Err(PullRequestMismatch::BaseRepository);
    }
    if run.head_repository.as_ref().map(|repository| repository.id) != Some(head_id)
        || run_pull
            .head
            .repository
            .as_ref()
            .is_some_and(|repository| repository.id != head_id)
    {
        return Err(PullRequestMismatch::HeadRepository);
    }
    Ok((head_id, base_id))
}

fn verify_pull_request_refs(
    run: &WorkflowRun,
    run_pull: &WorkflowRunPullRequest,
    pull: &PullRequest,
) -> Result<(), PullRequestMismatch> {
    if run.head_branch.as_deref() != Some(pull.head.reference.as_str())
        || run.head_sha != pull.head.sha
        || run_pull.head.reference != pull.head.reference
        || run_pull.head.sha != pull.head.sha
    {
        return Err(PullRequestMismatch::Head);
    }
    if run_pull.base.reference != pull.base.reference || run_pull.base.sha != pull.base.sha {
        return Err(PullRequestMismatch::Base);
    }
    Ok(())
}
