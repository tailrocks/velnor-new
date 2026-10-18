//! Source checks for authenticated job metadata.

use crate::CompleteSourceMetadata;

use super::workflow_ref::workflow_source;
use super::{PullRequest, VerifiedJobWorkflowRef, WorkflowRun};

/// Expected repository identity for one runner connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepositoryTarget<'a> {
    /// Stable repository identifier.
    pub id: i64,
    /// GitHub account name.
    pub owner: &'a str,
    /// GitHub repository name.
    pub name: &'a str,
}

/// Repository identity returned only after an authenticated exact-scope lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRepositoryScope {
    id: i64,
    owner: String,
    name: String,
}

impl VerifiedRepositoryScope {
    /// Immutable repository ID from GitHub.
    #[must_use]
    pub const fn id(&self) -> i64 {
        self.id
    }

    /// Canonical owner login from GitHub.
    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// Canonical repository name from GitHub.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Repository identity returned only after an authenticated exact-scope lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRepositoryIdentity {
    id: i64,
    owner: String,
    name: String,
    private: bool,
}

impl VerifiedRepositoryIdentity {
    /// Immutable repository ID from GitHub.
    #[must_use]
    pub const fn id(&self) -> i64 {
        self.id
    }

    /// Canonical owner login from GitHub.
    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// Canonical repository name from GitHub.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether GitHub marks the repository private.
    #[must_use]
    pub const fn is_private(&self) -> bool {
        self.private
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RepositoryScopeMismatch {
    Id,
    Name,
    Public,
}

pub(super) fn verify_repository_scope(
    repository: &super::GitHubRepository,
    expected: RepositoryTarget<'_>,
) -> Result<VerifiedRepositoryScope, RepositoryScopeMismatch> {
    if expected.id <= 0 || repository.id != expected.id {
        return Err(RepositoryScopeMismatch::Id);
    }
    let full_name = format!("{}/{}", expected.owner, expected.name);
    if expected.owner.is_empty()
        || expected.name.is_empty()
        || !same_name(&repository.owner.login, expected.owner)
        || !same_name(&repository.name, expected.name)
        || !same_name(&repository.full_name, &full_name)
    {
        return Err(RepositoryScopeMismatch::Name);
    }
    if !repository.private {
        return Err(RepositoryScopeMismatch::Public);
    }
    Ok(VerifiedRepositoryScope {
        id: repository.id,
        owner: repository.owner.login.clone(),
        name: repository.name.clone(),
    })
}

pub(super) fn verify_repository_identity(
    repository: &super::GitHubRepository,
    expected: RepositoryTarget<'_>,
) -> Result<VerifiedRepositoryIdentity, RepositoryScopeMismatch> {
    if expected.id <= 0 || repository.id != expected.id {
        return Err(RepositoryScopeMismatch::Id);
    }
    let full_name = format!("{}/{}", expected.owner, expected.name);
    if expected.owner.is_empty()
        || expected.name.is_empty()
        || !same_name(&repository.owner.login, expected.owner)
        || !same_name(&repository.name, expected.name)
        || !same_name(&repository.full_name, &full_name)
    {
        return Err(RepositoryScopeMismatch::Name);
    }
    Ok(VerifiedRepositoryIdentity {
        id: repository.id,
        owner: repository.owner.login.clone(),
        name: repository.name.clone(),
        private: repository.private,
    })
}

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

/// Pull request source values compared with a workflow run and API record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPullRequest {
    /// Pull request number.
    pub number: i64,
    /// Head repository identifier. A fork has a different identifier.
    pub head_repository_id: i64,
    /// Base repository identifier.
    pub base_repository_id: i64,
    /// Head branch name.
    pub head_ref: String,
    /// Head branch commit.
    pub head_sha: String,
    /// Base branch name.
    pub base_ref: String,
    /// Base branch commit.
    pub base_sha: String,
    /// Workflow run head commit.
    pub run_head_sha: String,
    /// Current pull request merge commit, when GitHub provides one.
    ///
    /// The workflow run record does not prove that the run used this commit.
    pub merge_commit_sha: Option<String>,
}

/// Pull request identity tied to an immutable workflow run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPullRequestRunAssociation {
    /// Pull request number from the workflow ref and run record.
    pub number: i64,
    /// Base repository ID from the immutable workflow run.
    pub base_repository_id: i64,
    /// Head repository ID from the immutable workflow run.
    pub head_repository_id: i64,
    /// Workflow run commit. Its role requires a separate Git graph check.
    pub run_head_sha: String,
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

/// Pull request values that are different from the workflow run values or API record values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PullRequestMismatch {
    /// The event, number, or expected repository is invalid.
    #[error("pull request event or number is different")]
    Event,
    /// The pull request number is different from the number in the run record.
    #[error("pull request number is different")]
    Number,
    /// The head repository is missing or is different.
    #[error("pull request head repository is different")]
    HeadRepository,
    /// The base repository is missing or is different.
    #[error("pull request base repository is different")]
    BaseRepository,
    /// The head ref or SHA is different.
    #[error("pull request head is different")]
    Head,
    /// The base ref or SHA is different.
    #[error("pull request base is different")]
    Base,
    /// A pull request commit SHA is malformed.
    #[error("pull request commit SHA is invalid")]
    CommitSha,
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

/// Compare pull request values with the workflow run and expected base repository.
///
/// # Errors
///
/// Gives [`PullRequestMismatch`] when a head or base value is invalid or different.
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

/// Verify the immutable run, repository, fork, and pull request association.
///
/// This function does not compare mutable head or base refs from the run's PR snapshot.
///
/// # Errors
///
/// Gives [`PullRequestMismatch`] when the run does not identify the exact PR scope.
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
) -> Result<&'a super::WorkflowRunPullRequest, PullRequestMismatch> {
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
    run_pull: &super::WorkflowRunPullRequest,
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
    run_pull: &super::WorkflowRunPullRequest,
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

fn same_name(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn workflow_ref_matches_run(run: &WorkflowRun, workflow: &VerifiedJobWorkflowRef) -> bool {
    match workflow.kind {
        super::WorkflowRefKind::Branch => workflow
            .reference
            .strip_prefix("refs/heads/")
            .is_some_and(|branch| {
                run.event != "pull_request" && run.head_branch.as_deref() == Some(branch)
            }),
        super::WorkflowRefKind::Tag => {
            matches!(run.event.as_str(), "push" | "release" | "workflow_dispatch")
                && workflow
                    .reference
                    .strip_prefix("refs/tags/")
                    .is_some_and(|tag| run.head_branch.as_deref() == Some(tag))
        }
        super::WorkflowRefKind::PullRequest => {
            run.event == "pull_request" && pull_request_ref_matches(run, &workflow.reference)
        }
        super::WorkflowRefKind::MergeQueue => run.event == "merge_group",
        super::WorkflowRefKind::Commit => workflow.reference == run.head_sha,
        super::WorkflowRefKind::Other => false,
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

fn valid_commit_sha(sha: &str) -> bool {
    matches!(sha.len(), 40 | 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}
