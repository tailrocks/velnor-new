//! Create one content-empty commit carrying the durable nonce marker.

use super::{
    GitCommit, GitCommitIdentity, RepositoryTarget, WorkflowRunError, api_headers, get_git_commit,
    get_repository_identity, repository_path,
};
use crate::TransportFail;
use crate::session::{Exchange, Method, SessionRequest, Transport};

const NONCE_PREFIX: &str = "velnor-pr-validation-nonce:v1:";

/// Exact commit-object response for one nonce publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestNonceCommitReceipt {
    /// Immutable head repository ID.
    pub head_repository_id: i64,
    /// Head branch to which the commit can be published.
    pub head_ref: String,
    /// Exact parent commit captured before the retarget acknowledgement.
    pub parent_sha: String,
    /// Random nonce embedded in the commit message.
    pub nonce: String,
    /// Explicit configured author and committer identity.
    pub identity: GitCommitIdentity,
    /// Created commit object.
    pub commit: GitCommit,
}

/// Create a new commit object that carries a nonce and directly extends one exact head commit.
///
/// This sends one POST request and does not retry. The caller must commit a durable creation claim
/// before calling this function. A timeout can follow object creation; the caller must record that
/// uncertainty and leave it sticky because a lost response may omit the returned commit SHA.
///
/// # Errors
///
/// Gives [`WorkflowRunError`] when the repository, input, or response is different from the
/// requested commit.
pub fn create_pull_request_nonce_commit<T>(
    transport: &mut T,
    head_repository: RepositoryTarget<'_>,
    head_ref: &str,
    parent_sha: &str,
    nonce: &str,
    commit_identity: &GitCommitIdentity,
    token: &str,
) -> Result<PullRequestNonceCommitReceipt, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    if !valid_branch(head_ref) || !valid_sha(parent_sha) || !valid_nonce(nonce) {
        return Err(WorkflowRunError::InvalidRequest);
    }
    let repository_identity = get_repository_identity(transport, head_repository, token)?;
    let parent = get_git_commit(
        transport,
        repository_identity.owner(),
        repository_identity.name(),
        parent_sha,
        token,
    )?;
    let message = format!(
        "Velnor pull request validation\n\n{NONCE_PREFIX}{nonce}\n\n{}",
        commit_identity.sign_off_line()
    );
    let body = serde_json::json!({
        "message": message,
        "tree": parent.tree.sha.clone(),
        "parents": [parent_sha],
        "author": { "name": commit_identity.name(), "email": commit_identity.email() },
        "committer": { "name": commit_identity.name(), "email": commit_identity.email() }
    })
    .to_string()
    .into_bytes();
    let path = repository_path(
        repository_identity.owner(),
        repository_identity.name(),
        "git/commits",
    )?;
    let mut headers = api_headers(token)?;
    headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
    let exchange = send_once(
        transport,
        &SessionRequest {
            method: Method::Post,
            path,
            query: None,
            headers,
            body,
        },
    )?;
    let commit = decode_commit(
        &exchange,
        parent_sha,
        &message,
        &parent.tree.sha,
        commit_identity,
    )?;
    let after = get_repository_identity(transport, head_repository, token)
        .map_err(|_| WorkflowRunError::Uncertain)?;
    if after.id() != repository_identity.id()
        || after.owner() != repository_identity.owner()
        || after.name() != repository_identity.name()
    {
        return Err(WorkflowRunError::Uncertain);
    }
    Ok(PullRequestNonceCommitReceipt {
        head_repository_id: repository_identity.id(),
        head_ref: head_ref.to_owned(),
        parent_sha: parent_sha.to_owned(),
        nonce: nonce.to_owned(),
        identity: commit_identity.clone(),
        commit,
    })
}

fn decode_commit(
    exchange: &Exchange,
    parent_sha: &str,
    message: &str,
    tree_sha: &str,
    identity: &GitCommitIdentity,
) -> Result<GitCommit, WorkflowRunError> {
    if exchange.status != 201 {
        return Err(super::api_status(exchange.status));
    }
    if exchange.body.len() > super::MAX_RESPONSE_BYTES {
        return Err(WorkflowRunError::Uncertain);
    }
    let commit: GitCommit =
        serde_json::from_slice(&exchange.body).map_err(|_| WorkflowRunError::Uncertain)?;
    if !valid_sha(&commit.sha)
        || commit.message != message
        || commit.tree.sha != tree_sha
        || commit.parents.len() != 1
        || commit.parents[0].sha != parent_sha
        || !person_matches(commit.author.as_ref(), identity)
        || !person_matches(commit.committer.as_ref(), identity)
    {
        return Err(WorkflowRunError::Uncertain);
    }
    Ok(commit)
}

fn person_matches(person: Option<&super::GitCommitPerson>, identity: &GitCommitIdentity) -> bool {
    person.is_some_and(|person| identity.matches_fields(&person.name, &person.email))
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
