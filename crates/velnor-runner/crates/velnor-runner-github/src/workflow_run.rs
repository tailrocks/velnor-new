//! GitHub workflow and pull request source records.

mod base_update;
mod commit_identity;
mod execution;
mod git_commit;
mod nonce_commit;
mod reference;
mod source;
mod timeline;
mod workflow_ref;

use source::{verify_repository_identity, verify_repository_scope};

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::TransportFail;
use crate::session::{Exchange, Method, SessionRequest, Transport};

pub use base_update::{
    PullRequestBaseUpdateReceipt, PullRequestBaseUpdateRequest, update_pull_request_base,
};
pub use commit_identity::{GitCommitIdentity, GitCommitIdentityError};
pub use execution::{
    PullRequestExecutionContext, PullRequestExecutionError, PullRequestExecutionMismatch,
    PullRequestNoncePublication, PullRequestRetargetAcknowledgement, VerifiedPullRequestExecution,
    verify_pull_request_execution,
};
pub use git_commit::{
    GitCommit, GitCommitParent, GitCommitPerson, GitCommitTree, get_git_commit, is_commit_ancestor,
};
pub use nonce_commit::{PullRequestNonceCommitReceipt, create_pull_request_nonce_commit};
pub use reference::{GitReference, GitReferenceObject, publish_pull_request_nonce_commit};
pub use source::{
    PullRequestMismatch, RepositoryTarget, SourceMismatch, VerifiedPullRequest,
    VerifiedPullRequestRunAssociation, VerifiedRepositoryIdentity, VerifiedRepositoryScope,
    VerifiedWorkflowSource, verify_pull_request, verify_pull_request_run_association,
    verify_workflow_source,
};
pub use timeline::{
    BaseRefChangedEvent, PullRequestBaseHistory, get_pull_request_base_history, new_retarget_event,
    ordered_base_changes, retarget_event_after_acknowledgement,
};
pub use workflow_ref::{VerifiedJobWorkflowRef, WorkflowRefKind};

/// GitHub REST API version used by source lookups.
pub const GITHUB_API_VERSION: &str = "2026-03-10";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// GitHub API request or source check failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WorkflowRunError {
    /// An input cannot form a GitHub API request.
    #[error("invalid GitHub API request")]
    InvalidRequest,
    /// The request did not return a known result.
    #[error("GitHub API result is uncertain")]
    Uncertain,
    /// GitHub returned an HTTP status other than 200.
    #[error("GitHub API returned status {0}")]
    HttpStatus(u16),
    /// The response did not match the GitHub API record.
    #[error("malformed GitHub API response")]
    Malformed,
    /// The response exceeded the local byte limit.
    #[error("GitHub API response is too large")]
    ResponseTooLarge,
    /// The repository endpoint did not match the configured private repository.
    #[error("GitHub repository identity did not match")]
    RepositoryMismatch,
}

/// GitHub account in a repository record.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GitHubAccount {
    /// Account login.
    pub login: String,
}

/// Repository record returned by the GitHub API.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GitHubRepository {
    /// Stable repository identifier.
    pub id: i64,
    /// Repository name.
    pub name: String,
    /// Owner and repository name.
    pub full_name: String,
    /// Repository owner.
    pub owner: GitHubAccount,
    /// Whether the repository is private. Missing values fail closed as public.
    #[serde(default)]
    pub private: bool,
}

/// Workflow used by a completed or active GitHub Actions run.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WorkflowRun {
    /// Workflow run identifier.
    pub id: i64,
    /// Event that started the workflow run.
    pub event: String,
    /// Workflow path and ref.
    pub path: String,
    /// Workflow run head branch, when GitHub returns one.
    #[serde(default)]
    pub head_branch: Option<String>,
    /// Workflow run head commit.
    pub head_sha: String,
    /// Workflow identifier.
    pub workflow_id: i64,
    /// Workflow run attempt number.
    pub run_attempt: i64,
    /// Repository that owns the workflow run.
    pub repository: GitHubRepository,
    /// Head repository when GitHub returns one.
    #[serde(default)]
    pub head_repository: Option<GitHubRepository>,
    /// Pull requests linked to this run.
    #[serde(default)]
    pub pull_requests: Vec<WorkflowRunPullRequest>,
    /// Reusable workflows recorded for this run.
    #[serde(default)]
    pub referenced_workflows: Vec<ReferencedWorkflow>,
}

/// Pull request reference included with a workflow run.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WorkflowRunPullRequest {
    /// Pull request number.
    pub number: i64,
    /// Pull request head branch.
    pub head: WorkflowRunPullRequestBranch,
    /// Pull request base branch.
    pub base: WorkflowRunPullRequestBranch,
}

/// Repository identity embedded in a workflow run pull request.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WorkflowRunRepositoryRef {
    /// Stable repository identifier.
    pub id: i64,
    /// Repository name.
    pub name: String,
}

/// Branch record embedded in a workflow run pull request.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WorkflowRunPullRequestBranch {
    /// Branch name.
    #[serde(rename = "ref")]
    pub reference: String,
    /// Branch commit.
    pub sha: String,
    /// Repository reference, when GitHub returns one.
    #[serde(default, rename = "repo")]
    pub repository: Option<WorkflowRunRepositoryRef>,
}

/// Workflow called by a run.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ReferencedWorkflow {
    /// Workflow path and ref.
    pub path: String,
    /// Source commit for this workflow.
    pub sha: String,
    /// Git ref for this workflow, when returned.
    #[serde(default, rename = "ref")]
    pub reference: Option<String>,
}

/// Branch record in a pull request.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PullRequestBranch {
    /// Branch name.
    #[serde(rename = "ref")]
    pub reference: String,
    /// Branch commit.
    pub sha: String,
    /// Repository for this branch, when it still exists.
    #[serde(default, rename = "repo")]
    pub repository: Option<GitHubRepository>,
}

/// Pull request returned by the GitHub API.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PullRequest {
    /// Pull request number.
    pub number: i64,
    /// Pull request state.
    pub state: String,
    /// Pull request head branch.
    pub head: PullRequestBranch,
    /// Pull request base branch.
    pub base: PullRequestBranch,
    /// Current pull request merge commit, when GitHub provides one.
    ///
    /// The workflow run record does not prove that the run used this commit.
    #[serde(default)]
    pub merge_commit_sha: Option<String>,
}

/// Get one workflow run from the GitHub REST API.
///
/// # Errors
///
/// Returns [`WorkflowRunError`] when the input, transport, status, or response is invalid.
pub fn get_workflow_run<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    workflow_run_id: i64,
    token: &str,
) -> Result<WorkflowRun, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    if workflow_run_id <= 0 {
        return Err(WorkflowRunError::InvalidRequest);
    }
    let path = repository_path(
        owner,
        repository,
        &format!("actions/runs/{workflow_run_id}"),
    )?;
    get_json(transport, path, token)
}

/// Authenticate the configured repository name and return its immutable private scope.
///
/// # Errors
///
/// Returns [`WorkflowRunError`] when the endpoint, token, or repository identity is invalid.
pub fn get_repository_scope<T>(
    transport: &mut T,
    expected: RepositoryTarget<'_>,
    token: &str,
) -> Result<VerifiedRepositoryScope, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    if expected.id <= 0 {
        return Err(WorkflowRunError::InvalidRequest);
    }
    let path = repository_path(expected.owner, expected.name, "")?;
    let repository: GitHubRepository = get_json(transport, path, token)?;
    verify_repository_scope(&repository, expected).map_err(|_| WorkflowRunError::RepositoryMismatch)
}

/// Authenticate an exact repository identity without assuming its visibility.
///
/// This is used for a pull request head repository, which may be a fork. The base repository
/// still uses [`get_repository_scope`] and must be private.
///
/// # Errors
///
/// Gives [`WorkflowRunError`] when the endpoint, token, or repository identity is incorrect.
pub fn get_repository_identity<T>(
    transport: &mut T,
    expected: RepositoryTarget<'_>,
    token: &str,
) -> Result<VerifiedRepositoryIdentity, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    if expected.id <= 0 {
        return Err(WorkflowRunError::InvalidRequest);
    }
    let path = repository_path(expected.owner, expected.name, "")?;
    let repository: GitHubRepository = get_json(transport, path, token)?;
    verify_repository_identity(&repository, expected)
        .map_err(|_| WorkflowRunError::RepositoryMismatch)
}

/// Get one pull request from the GitHub REST API.
///
/// # Errors
///
/// Returns [`WorkflowRunError`] when the input, transport, status, or response is invalid.
pub fn get_pull_request<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    pull_number: i64,
    token: &str,
) -> Result<PullRequest, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    if pull_number <= 0 {
        return Err(WorkflowRunError::InvalidRequest);
    }
    let path = repository_path(owner, repository, &format!("pulls/{pull_number}"))?;
    get_json(transport, path, token)
}

fn get_json<T, R>(transport: &mut T, path: String, token: &str) -> Result<R, WorkflowRunError>
where
    T: Transport + ?Sized,
    R: DeserializeOwned,
{
    let request = SessionRequest {
        method: Method::Get,
        path,
        query: None,
        headers: api_headers(token)?,
        body: Vec::new(),
    };
    let exchange = match transport.exchange(&request) {
        Ok(exchange) => exchange,
        Err(TransportFail::Timeout | TransportFail::Reset) => {
            return Err(WorkflowRunError::Uncertain);
        }
        Err(TransportFail::Http(status)) => return Err(api_status(status)),
    };
    decode(&exchange)
}

fn decode<R: DeserializeOwned>(exchange: &Exchange) -> Result<R, WorkflowRunError> {
    if exchange.status != 200 {
        return Err(api_status(exchange.status));
    }
    if exchange.body.len() > MAX_RESPONSE_BYTES {
        return Err(WorkflowRunError::ResponseTooLarge);
    }
    serde_json::from_slice(&exchange.body).map_err(|_| WorkflowRunError::Malformed)
}

fn api_status(status: u16) -> WorkflowRunError {
    if status >= 500 {
        WorkflowRunError::Uncertain
    } else {
        WorkflowRunError::HttpStatus(status)
    }
}

fn api_headers(token: &str) -> Result<Vec<(String, String)>, WorkflowRunError> {
    if token.is_empty()
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\'))
    {
        return Err(WorkflowRunError::InvalidRequest);
    }
    Ok(vec![
        (
            "Accept".to_owned(),
            "application/vnd.github+json".to_owned(),
        ),
        ("Authorization".to_owned(), format!("Bearer {token}")),
        (
            "X-GitHub-Api-Version".to_owned(),
            GITHUB_API_VERSION.to_owned(),
        ),
        ("User-Agent".to_owned(), "velnor-host".to_owned()),
    ])
}

fn repository_path(owner: &str, repository: &str, tail: &str) -> Result<String, WorkflowRunError> {
    if !path_segment(owner) || !path_segment(repository) {
        return Err(WorkflowRunError::InvalidRequest);
    }
    if tail.is_empty() {
        Ok(format!("/repos/{owner}/{repository}"))
    } else {
        Ok(format!("/repos/{owner}/{repository}/{tail}"))
    }
}

fn path_segment(value: &str) -> bool {
    !matches!(value, "." | "..")
        && !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}
