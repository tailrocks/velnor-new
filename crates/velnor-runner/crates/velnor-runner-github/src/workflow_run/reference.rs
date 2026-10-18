//! Publish one nonce commit to an exact existing head reference.

use serde::{Deserialize, Serialize};

use super::{
    PullRequestNonceCommitReceipt, RepositoryTarget, WorkflowRunError, api_headers, get_git_commit,
    get_repository_identity, repository_path,
};
use crate::TransportFail;
use crate::session::{Exchange, Method, SessionRequest, Transport};

/// GitHub reference returned by the Git database API.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GitReference {
    /// Full reference such as `refs/heads/feature`.
    #[serde(rename = "ref")]
    pub reference: String,
    /// Commit currently selected by the reference.
    pub object: GitReferenceObject,
}

/// Object selected by one GitHub reference.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GitReferenceObject {
    /// Object type returned by GitHub.
    #[serde(rename = "type")]
    pub object_type: String,
    /// Selected commit SHA.
    pub sha: String,
}

#[derive(Serialize)]
struct UpdateReferenceBody {
    sha: String,
    force: bool,
}

/// Move the exact head branch to a previously created nonce commit.
///
/// This sends one PATCH request with `force: false` and does not retry. The caller must commit a
/// durable publication claim before calling it. Timeout and malformed success responses require
/// read-only reconciliation; do not repeat the PATCH after an uncertain result.
///
/// # Errors
///
/// Gives [`WorkflowRunError`] when the repository, old ref, nonce commit, or update response is
/// different from the requested publication.
pub fn publish_pull_request_nonce_commit<T>(
    transport: &mut T,
    head_repository: RepositoryTarget<'_>,
    receipt: &PullRequestNonceCommitReceipt,
    token: &str,
) -> Result<GitReference, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    validate_receipt(head_repository, receipt)?;
    let identity = get_repository_identity(transport, head_repository, token)?;
    if identity.id() != receipt.head_repository_id {
        return Err(WorkflowRunError::RepositoryMismatch);
    }
    let commit = get_git_commit(
        transport,
        identity.owner(),
        identity.name(),
        &receipt.commit.sha,
        token,
    )?;
    if commit != receipt.commit
        || commit.parents.len() != 1
        || commit.parents[0].sha != receipt.parent_sha
        || !commit
            .message
            .lines()
            .any(|line| line == format!("velnor-pr-validation-nonce:v1:{}", receipt.nonce))
    {
        return Err(WorkflowRunError::Malformed);
    }
    let reference = format!("refs/heads/{}", receipt.head_ref);
    let current = get_reference(
        transport,
        identity.owner(),
        identity.name(),
        &receipt.head_ref,
        token,
    )?;
    if current.reference != reference
        || current.object.object_type != "commit"
        || current.object.sha != receipt.parent_sha
    {
        return Err(WorkflowRunError::RepositoryMismatch);
    }

    let path = repository_path(
        identity.owner(),
        identity.name(),
        &format!("git/refs/heads/{}", receipt.head_ref),
    )?;
    let body = serde_json::to_vec(&UpdateReferenceBody {
        sha: receipt.commit.sha.clone(),
        force: false,
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
    let updated = decode_updated_reference(&exchange)?;
    if updated.reference != reference
        || updated.object.object_type != "commit"
        || updated.object.sha != receipt.commit.sha
    {
        return Err(WorkflowRunError::Uncertain);
    }
    let after = get_repository_identity(transport, head_repository, token)
        .map_err(|_| WorkflowRunError::Uncertain)?;
    if after.id() != identity.id()
        || after.owner() != identity.owner()
        || after.name() != identity.name()
    {
        return Err(WorkflowRunError::Uncertain);
    }
    Ok(updated)
}

fn get_reference<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    branch: &str,
    token: &str,
) -> Result<GitReference, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    let path = repository_path(owner, repository, &format!("git/ref/heads/{branch}"))?;
    let request = SessionRequest {
        method: Method::Get,
        path,
        query: None,
        headers: api_headers(token)?,
        body: Vec::new(),
    };
    let exchange = send_once(transport, &request)?;
    decode_reference(&exchange)
}

fn decode_reference(exchange: &Exchange) -> Result<GitReference, WorkflowRunError> {
    if exchange.status != 200 {
        return Err(super::api_status(exchange.status));
    }
    if exchange.body.len() > super::MAX_RESPONSE_BYTES {
        return Err(WorkflowRunError::ResponseTooLarge);
    }
    serde_json::from_slice(&exchange.body).map_err(|_| WorkflowRunError::Malformed)
}

fn decode_updated_reference(exchange: &Exchange) -> Result<GitReference, WorkflowRunError> {
    if exchange.status != 200 {
        return Err(super::api_status(exchange.status));
    }
    if exchange.body.len() > super::MAX_RESPONSE_BYTES {
        return Err(WorkflowRunError::Uncertain);
    }
    serde_json::from_slice(&exchange.body).map_err(|_| WorkflowRunError::Uncertain)
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

fn validate_receipt(
    expected: RepositoryTarget<'_>,
    receipt: &PullRequestNonceCommitReceipt,
) -> Result<(), WorkflowRunError> {
    if expected.id <= 0
        || expected.id != receipt.head_repository_id
        || !valid_branch(&receipt.head_ref)
        || !valid_sha(&receipt.parent_sha)
        || !valid_sha(&receipt.commit.sha)
        || receipt.commit.parents.len() != 1
        || receipt.commit.parents[0].sha != receipt.parent_sha
        || !valid_nonce(&receipt.nonce)
    {
        return Err(WorkflowRunError::InvalidRequest);
    }
    Ok(())
}

fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_nonce(value: &str) -> bool {
    value.len() == 64
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
