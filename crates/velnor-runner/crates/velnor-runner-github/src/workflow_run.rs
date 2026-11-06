// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod attempt;
mod base_update;
mod commit_identity;
mod execution;
mod git_commit;
mod hook_source;
mod nonce_commit;
mod reference;
mod source;
mod timeline;
mod workflow_content;
mod workflow_ref;

use source::{verify_repository_identity, verify_repository_scope};

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::TransportFail;
use crate::session::{Exchange, Method, SessionRequest, Transport};

pub use attempt::get_workflow_run_attempt;
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
pub use hook_source::{
    ComparedHookPullRequestFacts, ComparedHookSourceFacts, HookSourceError,
    UntrustedHookPullRequest, UntrustedHookSourceInput, compare_hook_source,
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
pub use workflow_content::{VerifiedWorkflowContent, get_workflow_content};
pub use workflow_ref::{VerifiedJobWorkflowRef, WorkflowRefKind};

pub const GITHUB_API_VERSION: &str = "2026-03-10";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WorkflowRunError {
    #[error("invalid GitHub API request")]
    InvalidRequest,

    #[error("GitHub API result is uncertain")]
    Uncertain,

    #[error("GitHub API returned status {0}")]
    HttpStatus(u16),

    #[error("malformed GitHub API response")]
    Malformed,

    #[error("GitHub API response is too large")]
    ResponseTooLarge,

    #[error("GitHub repository identity did not match")]
    RepositoryMismatch,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GitHubAccount {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GitHubRepository {
    pub id: i64,

    pub name: String,

    pub full_name: String,

    pub owner: GitHubAccount,

    #[serde(default)]
    pub private: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WorkflowRun {
    pub id: i64,

    pub event: String,

    pub path: String,

    #[serde(default)]
    pub head_branch: Option<String>,

    pub head_sha: String,

    pub workflow_id: i64,

    pub run_attempt: i64,

    pub repository: GitHubRepository,

    #[serde(default)]
    pub head_repository: Option<GitHubRepository>,

    #[serde(default)]
    pub pull_requests: Vec<WorkflowRunPullRequest>,

    #[serde(default)]
    pub referenced_workflows: Vec<ReferencedWorkflow>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WorkflowRunPullRequest {
    pub number: i64,

    pub head: WorkflowRunPullRequestBranch,

    pub base: WorkflowRunPullRequestBranch,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WorkflowRunRepositoryRef {
    pub id: i64,

    pub name: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WorkflowRunPullRequestBranch {
    #[serde(rename = "ref")]
    pub reference: String,

    pub sha: String,

    #[serde(default, rename = "repo")]
    pub repository: Option<WorkflowRunRepositoryRef>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ReferencedWorkflow {
    pub path: String,

    pub sha: String,

    #[serde(default, rename = "ref")]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PullRequestBranch {
    #[serde(rename = "ref")]
    pub reference: String,

    pub sha: String,

    #[serde(default, rename = "repo")]
    pub repository: Option<GitHubRepository>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PullRequest {
    pub number: i64,

    pub state: String,

    pub merged: bool,

    pub head: PullRequestBranch,

    pub base: PullRequestBranch,

    #[serde(default)]
    pub merge_commit_sha: Option<String>,
}

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
