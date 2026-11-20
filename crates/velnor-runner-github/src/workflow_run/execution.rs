// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    GitCommit, GitCommitIdentity, PullRequestBaseHistory, PullRequestMismatch, RepositoryTarget,
    VerifiedPullRequestRunAssociation, VerifiedWorkflowSource, WorkflowRefKind, WorkflowRun,
    WorkflowRunError, get_git_commit, is_commit_ancestor, retarget_event_after_acknowledgement,
    verify_pull_request_run_association, verify_workflow_source,
};
use crate::CompleteSourceMetadata;
use crate::session::Transport;

mod validation;
use validation::{parse_pull_request_number, repository_parts, valid_nonce, valid_sha};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestRetargetAcknowledgement {
    pub number: i64,

    pub base_repository_id: i64,

    pub head_repository_id: i64,

    pub head_ref: String,

    pub head_sha: String,

    pub previous_base_ref: String,

    pub current_base_ref: String,

    pub event_id: String,

    pub base_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestNoncePublication {
    pub nonce: String,

    pub commit_sha: String,

    pub identity: GitCommitIdentity,
}

#[derive(Debug, Clone, Copy)]
pub struct PullRequestExecutionContext<'a> {
    pub expected: RepositoryTarget<'a>,

    pub expected_head_repository_id: i64,

    pub expected_head_ref: &'a str,

    pub expected_base_ref: &'a str,

    pub history: &'a PullRequestBaseHistory,

    pub acknowledgement: &'a PullRequestRetargetAcknowledgement,

    pub publication: &'a PullRequestNoncePublication,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPullRequestExecution {
    association: VerifiedPullRequestRunAssociation,
    run_base_sha: Option<String>,
    retarget_event_id: String,
    nonce_commit_sha: String,
}

impl VerifiedPullRequestExecution {
    #[must_use]
    pub const fn association(&self) -> &VerifiedPullRequestRunAssociation {
        &self.association
    }

    #[must_use]
    pub fn run_base_sha(&self) -> Option<&str> {
        self.run_base_sha.as_deref()
    }

    #[must_use]
    pub fn retarget_event_id(&self) -> &str {
        &self.retarget_event_id
    }

    #[must_use]
    pub fn nonce_commit_sha(&self) -> &str {
        &self.nonce_commit_sha
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PullRequestExecutionMismatch {
    #[error("workflow run source is different")]
    WorkflowSource,

    #[error("pull request acknowledgement is different")]
    Acknowledgement,

    #[error("pull request base history is different")]
    BaseHistory,

    #[error("pull request nonce head is different")]
    NonceHead,

    #[error("pull request nonce commit is different")]
    NonceCommit,

    #[error("workflow run commit ancestry is different")]
    RunCommit,

    #[error("pull request base commit ancestry is different")]
    BaseAncestry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PullRequestExecutionError {
    #[error(transparent)]
    Association(#[from] PullRequestMismatch),

    #[error(transparent)]
    Api(#[from] WorkflowRunError),

    #[error(transparent)]
    Proof(#[from] PullRequestExecutionMismatch),
}

pub fn verify_pull_request_execution<T>(
    transport: &mut T,
    run: &WorkflowRun,
    source: &VerifiedWorkflowSource,
    context: PullRequestExecutionContext<'_>,
    token: &str,
) -> Result<VerifiedPullRequestExecution, PullRequestExecutionError>
where
    T: Transport + ?Sized,
{
    let number = parse_pull_request_number(&source.job_workflow.reference)
        .ok_or(PullRequestExecutionMismatch::WorkflowSource)?;
    let source_from_run = verify_workflow_source(
        CompleteSourceMetadata {
            owner_name: &source.owner_name,
            repository_name: &source.repository_name,
            job_workflow_ref: &source.job_workflow_ref,
            workflow_run_id: source.workflow_run_id,
            event_name: &source.event_name,
        },
        run,
        context.expected,
    )
    .map_err(|_| PullRequestExecutionMismatch::WorkflowSource)?;
    let association = verify_pull_request_run_association(
        run,
        number,
        context.expected.id,
        context.expected_head_repository_id,
    )?;
    if source.repository_id != context.expected.id
        || &source_from_run != source
        || source.workflow_run_id != run.id
        || source.event_name != "pull_request"
        || source.head_sha != run.head_sha
        || source.job_workflow.kind != WorkflowRefKind::PullRequest
        || source.job_workflow.reference != format!("refs/pull/{number}/merge")
    {
        return Err(PullRequestExecutionMismatch::WorkflowSource.into());
    }
    verify_acknowledgement(
        context.expected,
        context.expected_head_repository_id,
        context.expected_head_ref,
        context.expected_base_ref,
        number,
        context.history,
        context.acknowledgement,
    )?;
    verify_publication(
        context.history,
        context.acknowledgement,
        context.publication,
    )?;

    let (owner, repository) = repository_parts(context.expected)?;
    let nonce_commit = get_git_commit(
        transport,
        owner,
        repository,
        &context.publication.commit_sha,
        token,
    )?;
    verify_nonce_commit(&nonce_commit, context.acknowledgement, context.publication)?;
    let run_commit = if run.head_sha == nonce_commit.sha {
        nonce_commit.clone()
    } else {
        get_git_commit(transport, owner, repository, &run.head_sha, token)?
    };
    let run_base_sha = verify_run_commit(&run_commit, &nonce_commit.sha)?;
    verify_base_ancestry(
        transport,
        owner,
        repository,
        context.acknowledgement,
        context.history,
        run_base_sha.as_deref(),
        token,
    )?;

    Ok(VerifiedPullRequestExecution {
        association,
        run_base_sha,
        retarget_event_id: context.acknowledgement.event_id.clone(),
        nonce_commit_sha: context.publication.commit_sha.clone(),
    })
}

fn verify_acknowledgement(
    expected: RepositoryTarget<'_>,
    expected_head_repository_id: i64,
    expected_head_ref: &str,
    expected_base_ref: &str,
    number: i64,
    history: &PullRequestBaseHistory,
    acknowledgement: &PullRequestRetargetAcknowledgement,
) -> Result<(), PullRequestExecutionMismatch> {
    if acknowledgement.number != number
        || acknowledgement.base_repository_id != expected.id
        || acknowledgement.head_repository_id != expected_head_repository_id
        || acknowledgement.head_ref != expected_head_ref
        || acknowledgement.current_base_ref != expected_base_ref
        || acknowledgement.previous_base_ref == acknowledgement.current_base_ref
        || !valid_sha(&acknowledgement.head_sha)
        || !valid_sha(&acknowledgement.base_sha)
        || acknowledgement.event_id.is_empty()
    {
        return Err(PullRequestExecutionMismatch::Acknowledgement);
    }
    if history.number != number
        || history.repository_id != expected.id
        || history.head_repository_id != expected_head_repository_id
        || history.base_repository_id != expected.id
        || history.head_ref != acknowledgement.head_ref
        || history.base_ref != acknowledgement.current_base_ref
        || !valid_sha(&history.head_sha)
        || !valid_sha(&history.base_sha)
    {
        return Err(PullRequestExecutionMismatch::BaseHistory);
    }
    retarget_event_after_acknowledgement(
        history,
        &acknowledgement.event_id,
        &acknowledgement.previous_base_ref,
        &acknowledgement.current_base_ref,
    )
    .map_err(|_| PullRequestExecutionMismatch::BaseHistory)?;
    Ok(())
}

fn verify_publication(
    history: &PullRequestBaseHistory,
    acknowledgement: &PullRequestRetargetAcknowledgement,
    publication: &PullRequestNoncePublication,
) -> Result<(), PullRequestExecutionMismatch> {
    if !valid_nonce(&publication.nonce)
        || !valid_sha(&publication.commit_sha)
        || history.head_sha != publication.commit_sha
    {
        return Err(PullRequestExecutionMismatch::NonceHead);
    }
    if acknowledgement.head_sha == publication.commit_sha {
        return Err(PullRequestExecutionMismatch::NonceCommit);
    }
    Ok(())
}

fn verify_nonce_commit(
    nonce_commit: &GitCommit,
    acknowledgement: &PullRequestRetargetAcknowledgement,
    publication: &PullRequestNoncePublication,
) -> Result<(), PullRequestExecutionMismatch> {
    let marker = format!("velnor-pr-validation-nonce:v1:{}", publication.nonce);
    if nonce_commit.sha != publication.commit_sha
        || nonce_commit.parents.len() != 1
        || nonce_commit.parents[0].sha != acknowledgement.head_sha
        || !nonce_commit.message.lines().any(|line| line == marker)
        || !nonce_commit
            .message
            .lines()
            .any(|line| line == publication.identity.sign_off_line())
        || !commit_person_matches(nonce_commit.author.as_ref(), &publication.identity)
        || !commit_person_matches(nonce_commit.committer.as_ref(), &publication.identity)
    {
        return Err(PullRequestExecutionMismatch::NonceCommit);
    }
    Ok(())
}

fn commit_person_matches(
    person: Option<&super::GitCommitPerson>,
    identity: &GitCommitIdentity,
) -> bool {
    person.is_some_and(|person| identity.matches_fields(&person.name, &person.email))
}

fn verify_run_commit(
    run_commit: &GitCommit,
    nonce_commit_sha: &str,
) -> Result<Option<String>, PullRequestExecutionMismatch> {
    if run_commit.sha == nonce_commit_sha {
        return Ok(None);
    }
    if run_commit.parents.len() != 2 || run_commit.parents[1].sha != nonce_commit_sha {
        return Err(PullRequestExecutionMismatch::RunCommit);
    }
    Ok(Some(run_commit.parents[0].sha.clone()))
}

fn verify_base_ancestry<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    acknowledgement: &PullRequestRetargetAcknowledgement,
    history: &PullRequestBaseHistory,
    run_base_sha: Option<&str>,
    token: &str,
) -> Result<(), PullRequestExecutionError>
where
    T: Transport + ?Sized,
{
    if let Some(run_base_sha) = run_base_sha {
        if !is_commit_ancestor(
            transport,
            owner,
            repository,
            &acknowledgement.base_sha,
            run_base_sha,
            token,
        )? || !is_commit_ancestor(
            transport,
            owner,
            repository,
            run_base_sha,
            &history.base_sha,
            token,
        )? {
            return Err(PullRequestExecutionMismatch::BaseAncestry.into());
        }
    } else if !is_commit_ancestor(
        transport,
        owner,
        repository,
        &acknowledgement.base_sha,
        &history.base_sha,
        token,
    )? {
        return Err(PullRequestExecutionMismatch::BaseAncestry.into());
    }
    Ok(())
}
