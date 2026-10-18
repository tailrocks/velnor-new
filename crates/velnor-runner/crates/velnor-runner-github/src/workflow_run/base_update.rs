//! One exact pull request base-ref update request.

use serde::{Deserialize, Serialize};

use super::{
    PullRequest, RepositoryTarget, WorkflowRunError, api_headers, get_pull_request,
    get_repository_scope, repository_path,
};
use crate::TransportFail;
use crate::session::{Exchange, Method, SessionRequest, Transport};

/// Exact pull request snapshot required before a base-ref update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestBaseUpdateRequest {
    /// Pull request number.
    pub number: i64,
    /// Expected head repository ID, including a fork ID when present.
    pub head_repository_id: i64,
    /// Exact head branch before the update.
    pub head_ref: String,
    /// Exact head commit before the update.
    pub head_sha: String,
    /// Current base branch before the update.
    pub previous_base_ref: String,
    /// Current base commit before the update.
    pub previous_base_sha: String,
    /// Target base branch.
    pub target_base_ref: String,
}

/// Exact pull request values returned by the base-ref update endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestBaseUpdateReceipt {
    request: PullRequestBaseUpdateRequest,
    target_base_sha: String,
}

impl PullRequestBaseUpdateReceipt {
    /// Exact request values acknowledged by GitHub.
    #[must_use]
    pub const fn request(&self) -> &PullRequestBaseUpdateRequest {
        &self.request
    }

    /// Base commit returned with the updated pull request.
    #[must_use]
    pub fn target_base_sha(&self) -> &str {
        &self.target_base_sha
    }
}

#[derive(Deserialize, Serialize)]
struct BaseUpdateBody {
    base: String,
}

/// Change one pull request's base branch after verifying the exact current snapshot.
///
/// This function sends one PATCH request and does not retry it. The caller must commit a durable
/// effect claim before calling it. A timeout or malformed success response can follow an applied
/// update, so the caller must record uncertainty and reconcile through read-only API calls.
///
/// # Errors
///
/// Gives [`WorkflowRunError`] when the repository, pull request snapshot, request, or response is
/// different from the requested transition.
pub fn update_pull_request_base<T>(
    transport: &mut T,
    expected: RepositoryTarget<'_>,
    request: PullRequestBaseUpdateRequest,
    token: &str,
) -> Result<PullRequestBaseUpdateReceipt, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    validate_request(expected, &request)?;
    get_repository_scope(transport, expected, token)?;
    let before = get_pull_request(
        transport,
        expected.owner,
        expected.name,
        request.number,
        token,
    )?;
    if !matches_before(&before, expected.id, &request) {
        return Err(WorkflowRunError::RepositoryMismatch);
    }
    let path = repository_path(
        expected.owner,
        expected.name,
        &format!("pulls/{}", request.number),
    )?;
    let body = serde_json::to_vec(&BaseUpdateBody {
        base: request.target_base_ref.clone(),
    })
    .map_err(|_| WorkflowRunError::InvalidRequest)?;
    let mut headers = api_headers(token)?;
    headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
    let exchange = send_once(
        transport,
        &SessionRequest {
            method: Method::Patch,
            path,
            query: None,
            headers,
            body,
        },
    )?;
    let receipt = decode_receipt(&exchange, expected.id, request)?;
    if get_repository_scope(transport, expected, token).is_err() {
        return Err(WorkflowRunError::Uncertain);
    }
    Ok(receipt)
}

fn validate_request(
    expected: RepositoryTarget<'_>,
    request: &PullRequestBaseUpdateRequest,
) -> Result<(), WorkflowRunError> {
    if expected.id <= 0
        || request.number <= 0
        || request.head_repository_id <= 0
        || !valid_branch(&request.head_ref)
        || !valid_branch(&request.previous_base_ref)
        || !valid_branch(&request.target_base_ref)
        || request.previous_base_ref == request.target_base_ref
        || !valid_sha(&request.head_sha)
        || !valid_sha(&request.previous_base_sha)
    {
        return Err(WorkflowRunError::InvalidRequest);
    }
    Ok(())
}

fn matches_before(
    pull: &PullRequest,
    base_repository_id: i64,
    request: &PullRequestBaseUpdateRequest,
) -> bool {
    pull.number == request.number
        && pull.state == "open"
        && pull.head.reference == request.head_ref
        && pull.head.sha == request.head_sha
        && pull.base.reference == request.previous_base_ref
        && pull.base.sha == request.previous_base_sha
        && pull
            .head
            .repository
            .as_ref()
            .is_some_and(|repository| repository.id == request.head_repository_id)
        && pull
            .base
            .repository
            .as_ref()
            .is_some_and(|repository| repository.id == base_repository_id)
}

fn decode_receipt(
    exchange: &Exchange,
    base_repository_id: i64,
    request: PullRequestBaseUpdateRequest,
) -> Result<PullRequestBaseUpdateReceipt, WorkflowRunError> {
    if exchange.status != 200 {
        return Err(super::api_status(exchange.status));
    }
    if exchange.body.len() > super::MAX_RESPONSE_BYTES {
        return Err(WorkflowRunError::Uncertain);
    }
    let pull: PullRequest =
        serde_json::from_slice(&exchange.body).map_err(|_| WorkflowRunError::Uncertain)?;
    if pull.number != request.number
        || pull.state != "open"
        || pull.head.reference != request.head_ref
        || pull.head.sha != request.head_sha
        || pull.base.reference != request.target_base_ref
        || !valid_sha(&pull.base.sha)
        || pull
            .head
            .repository
            .as_ref()
            .is_none_or(|repository| repository.id != request.head_repository_id)
        || pull
            .base
            .repository
            .as_ref()
            .is_none_or(|repository| repository.id != base_repository_id)
    {
        return Err(WorkflowRunError::Uncertain);
    }
    Ok(PullRequestBaseUpdateReceipt {
        request,
        target_base_sha: pull.base.sha,
    })
}

fn send_once<T>(transport: &mut T, request: &SessionRequest) -> Result<Exchange, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    match transport.exchange(request) {
        Ok(exchange) => Ok(exchange),
        Err(TransportFail::Timeout | TransportFail::Reset) => Err(WorkflowRunError::Uncertain),
        Err(TransportFail::Http(status)) => Err(super::api_status(status)),
    }
}

fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_branch(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 255
        || value.starts_with('/')
        || value.ends_with('/')
        || value.ends_with('.')
        || value.contains("..")
        || value.contains("//")
        || value.contains("@{")
        || value == "@"
    {
        return false;
    }
    value.split('/').all(|part| {
        !part.is_empty()
            && !part.starts_with('.')
            && !part.as_bytes().ends_with(b".lock")
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
    })
}
