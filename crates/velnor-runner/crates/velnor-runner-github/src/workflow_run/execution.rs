//! Causal proof for one branch-restricted pull request run.

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

/// Durable facts captured when the controller confirms a base-ref change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestRetargetAcknowledgement {
    /// Pull request number.
    pub number: i64,
    /// Immutable base repository ID.
    pub base_repository_id: i64,
    /// Immutable head repository ID.
    pub head_repository_id: i64,
    /// Head branch captured at the retarget acknowledgement.
    pub head_ref: String,
    /// Head commit captured before the nonce commit was created.
    pub head_sha: String,
    /// Base ref before the acknowledged change.
    pub previous_base_ref: String,
    /// Base ref after the acknowledged change.
    pub current_base_ref: String,
    /// Exact base-ref timeline event observed after the API acknowledgement.
    pub event_id: String,
    /// Base commit observed after the API acknowledgement.
    pub base_sha: String,
}

/// Durable nonce publication facts recorded after the retarget acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestNoncePublication {
    /// Unpredictable lowercase hexadecimal nonce written into the commit message.
    pub nonce: String,
    /// Exact commit object published to the pull request head ref.
    pub commit_sha: String,
    /// Configured identity that signed off the nonce commit.
    pub identity: GitCommitIdentity,
}

/// Exact policy and durable transition inputs for one run check.
#[derive(Debug, Clone, Copy)]
pub struct PullRequestExecutionContext<'a> {
    /// Authenticated target repository.
    pub expected: RepositoryTarget<'a>,
    /// Exact head repository ID from the configured pull request rule.
    pub expected_head_repository_id: i64,
    /// Exact head branch from the configured pull request rule.
    pub expected_head_ref: &'a str,
    /// Exact base branch from the configured pull request rule.
    pub expected_base_ref: &'a str,
    /// Complete base-ref history read from GitHub.
    pub history: &'a PullRequestBaseHistory,
    /// Durable acknowledgement read from the controller journal.
    pub acknowledgement: &'a PullRequestRetargetAcknowledgement,
    /// Durable nonce publication read from the controller journal.
    pub publication: &'a PullRequestNoncePublication,
}

/// Immutable pull request run facts with the required commit ancestry proved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPullRequestExecution {
    association: VerifiedPullRequestRunAssociation,
    run_base_sha: Option<String>,
    retarget_event_id: String,
    nonce_commit_sha: String,
}

impl VerifiedPullRequestExecution {
    /// Exact pull request association from the workflow run.
    #[must_use]
    pub const fn association(&self) -> &VerifiedPullRequestRunAssociation {
        &self.association
    }

    /// Base commit used by a synthetic merge run. Direct head runs have no base commit.
    #[must_use]
    pub fn run_base_sha(&self) -> Option<&str> {
        self.run_base_sha.as_deref()
    }

    /// Exact acknowledged base-ref event.
    #[must_use]
    pub fn retarget_event_id(&self) -> &str {
        &self.retarget_event_id
    }

    /// Exact random nonce commit published after the acknowledgement.
    #[must_use]
    pub fn nonce_commit_sha(&self) -> &str {
        &self.nonce_commit_sha
    }
}

/// Why one workflow run does not prove the acknowledged pull request context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PullRequestExecutionMismatch {
    /// The workflow run is different from the authenticated source facts.
    #[error("workflow run source is different")]
    WorkflowSource,
    /// The durable acknowledgement is different from the requested PR context.
    #[error("pull request acknowledgement is different")]
    Acknowledgement,
    /// The complete base-ref history is different from the acknowledgement.
    #[error("pull request base history is different")]
    BaseHistory,
    /// The current PR head is different from the nonce publication.
    #[error("pull request nonce head is different")]
    NonceHead,
    /// The nonce commit does not directly extend the captured head.
    #[error("pull request nonce commit is different")]
    NonceCommit,
    /// The run commit does not have the required head/base ancestry.
    #[error("workflow run commit ancestry is different")]
    RunCommit,
    /// A required base commit ancestry is absent.
    #[error("pull request base commit ancestry is different")]
    BaseAncestry,
}

/// Failure while checking a pull request run and its commit graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PullRequestExecutionError {
    /// The immutable run association is different.
    #[error(transparent)]
    Association(#[from] PullRequestMismatch),
    /// The GitHub API request did not return a trustworthy result.
    #[error(transparent)]
    Api(#[from] WorkflowRunError),
    /// The source facts do not establish the required causal context.
    #[error(transparent)]
    Proof(#[from] PullRequestExecutionMismatch),
}

/// Verify the PR source, completed base history, nonce commit, and run commit graph.
///
/// `acknowledgement` and `publication` must come from the durable controller journal. The
/// publication nonce must have been generated only after the acknowledgement was committed.
///
/// # Errors
///
/// Gives [`PullRequestExecutionError`] when any identity, API response, or ancestry check fails.
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
