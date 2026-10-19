//! Read workflow bytes from a specified commit in a private repository.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use sha1_smol::Sha1;
use sha2::{Digest, Sha256};

use super::{
    RepositoryTarget, WorkflowRunError, api_headers, get_git_commit, get_repository_scope,
    repository_path,
};
use crate::TransportFail;
use crate::session::{Exchange, Method, SessionRequest, Transport};

const MAX_WORKFLOW_BYTES: usize = 256 * 1024;
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Workflow bytes read from one specified private repository commit.
///
/// This value records source identity. It does not give approval for the workflow or job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedWorkflowContent {
    repository_id: i64,
    commit_sha: String,
    tree_sha: String,
    path: String,
    blob_sha: String,
    content_sha256: String,
    bytes: Vec<u8>,
}

impl VerifiedWorkflowContent {
    /// Stable repository ID returned by GitHub.
    #[must_use]
    pub const fn repository_id(&self) -> i64 {
        self.repository_id
    }

    /// Commit object ID supplied to the reader.
    #[must_use]
    pub fn commit_sha(&self) -> &str {
        &self.commit_sha
    }

    /// Root tree object ID for the commit.
    #[must_use]
    pub fn tree_sha(&self) -> &str {
        &self.tree_sha
    }

    /// Workflow path in the specified commit.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Git blob object ID for the workflow bytes.
    #[must_use]
    pub fn blob_sha(&self) -> &str {
        &self.blob_sha
    }

    /// SHA-256 of the decoded workflow bytes.
    #[must_use]
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }

    /// Workflow bytes. Maximum size: 256 KiB.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Read one workflow file from separate Git tree objects.
///
/// The repository response must have the configured ID, name, and private visibility. The caller
/// must compare the returned content identity with the workflow approval inventory.
///
/// # Errors
///
/// Gives [`WorkflowRunError`] when the repository, path, Git objects, transport, or content is
/// invalid.
pub fn get_workflow_content<T>(
    transport: &mut T,
    repository: RepositoryTarget<'_>,
    commit_sha: &str,
    workflow_path: &str,
    token: &str,
) -> Result<VerifiedWorkflowContent, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    let components = workflow_components(workflow_path)?;
    let scope = get_repository_scope(transport, repository, token)?;
    let commit = get_git_commit(
        transport,
        repository.owner,
        repository.name,
        commit_sha,
        token,
    )?;
    let commit_tree_sha = commit.tree.sha;
    let mut tree_sha = commit_tree_sha.clone();
    for component in &components[..2] {
        let tree = get_tree(transport, repository, &tree_sha, token)?;
        let entry = matching_entry(&tree, component)?;
        if entry.mode != "040000" || entry.kind != "tree" {
            return Err(WorkflowRunError::Malformed);
        }
        tree_sha.clone_from(&entry.sha);
    }
    let tree = get_tree(transport, repository, &tree_sha, token)?;
    let file = matching_entry(&tree, components[2])?;
    if !matches!(file.mode.as_str(), "100644" | "100755") || file.kind != "blob" {
        return Err(WorkflowRunError::Malformed);
    }
    let blob = get_blob(transport, repository, &file.sha, token)?;
    let bytes = decode_blob(&blob, &file.sha, file.size)?;
    let content_sha256 = digest_hex(Sha256::digest(&bytes).as_slice());
    Ok(VerifiedWorkflowContent {
        repository_id: scope.id(),
        commit_sha: commit.sha,
        tree_sha: commit_tree_sha,
        path: workflow_path.to_owned(),
        blob_sha: file.sha.clone(),
        content_sha256,
        bytes,
    })
}

fn workflow_components(path: &str) -> Result<[&str; 3], WorkflowRunError> {
    let Some((prefix, filename)) = path.rsplit_once('/') else {
        return Err(WorkflowRunError::InvalidRequest);
    };
    if prefix != ".github/workflows"
        || filename.is_empty()
        || filename == "."
        || filename == ".."
        || matches!(filename, ".yml" | ".yaml")
        || !std::path::Path::new(filename)
            .extension()
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
            })
        || !filename
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(WorkflowRunError::InvalidRequest);
    }
    Ok([".github", "workflows", filename])
}

#[derive(Debug, Deserialize)]
struct GitTree {
    sha: String,
    tree: Vec<GitTreeEntry>,
    truncated: bool,
}

#[derive(Debug, Deserialize)]
struct GitTreeEntry {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
    size: Option<usize>,
}

fn get_tree<T>(
    transport: &mut T,
    repository: RepositoryTarget<'_>,
    tree_sha: &str,
    token: &str,
) -> Result<GitTree, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    if !valid_git_sha(tree_sha) {
        return Err(WorkflowRunError::Malformed);
    }
    let path = repository_path(
        repository.owner,
        repository.name,
        &format!("git/trees/{tree_sha}"),
    )?;
    let tree: GitTree = get_json(transport, path, token)?;
    if tree.sha != tree_sha || !valid_git_sha(&tree.sha) || tree.truncated {
        return Err(WorkflowRunError::Malformed);
    }
    for entry in &tree.tree {
        if !valid_git_sha(&entry.sha) {
            return Err(WorkflowRunError::Malformed);
        }
    }
    Ok(tree)
}

fn matching_entry<'a>(tree: &'a GitTree, path: &str) -> Result<&'a GitTreeEntry, WorkflowRunError> {
    let mut matches = tree.tree.iter().filter(|entry| entry.path == path);
    let entry = matches.next().ok_or(WorkflowRunError::Malformed)?;
    if matches.next().is_some() {
        return Err(WorkflowRunError::Malformed);
    }
    Ok(entry)
}

#[derive(Debug, Deserialize)]
struct GitBlob {
    sha: String,
    encoding: String,
    size: usize,
    content: String,
}

fn get_blob<T>(
    transport: &mut T,
    repository: RepositoryTarget<'_>,
    blob_sha: &str,
    token: &str,
) -> Result<GitBlob, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    if !valid_git_sha(blob_sha) {
        return Err(WorkflowRunError::Malformed);
    }
    let path = repository_path(
        repository.owner,
        repository.name,
        &format!("git/blobs/{blob_sha}"),
    )?;
    get_json(transport, path, token)
}

fn decode_blob(
    blob: &GitBlob,
    expected_sha: &str,
    tree_size: Option<usize>,
) -> Result<Vec<u8>, WorkflowRunError> {
    if blob.sha != expected_sha || !valid_git_sha(&blob.sha) || blob.encoding != "base64" {
        return Err(WorkflowRunError::Malformed);
    }
    if blob.size > MAX_WORKFLOW_BYTES {
        return Err(WorkflowRunError::ResponseTooLarge);
    }
    if tree_size != Some(blob.size) {
        return Err(WorkflowRunError::Malformed);
    }
    let encoded = blob
        .content
        .bytes()
        .filter(|byte| !matches!(byte, b'\r' | b'\n'))
        .collect::<Vec<_>>();
    if encoded.len() > MAX_WORKFLOW_BYTES.div_ceil(3) * 4 {
        return Err(WorkflowRunError::ResponseTooLarge);
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| WorkflowRunError::Malformed)?;
    if bytes.len() != blob.size || bytes.len() > MAX_WORKFLOW_BYTES {
        return Err(WorkflowRunError::Malformed);
    }
    let mut git_blob = format!("blob {}\0", bytes.len()).into_bytes();
    git_blob.extend_from_slice(&bytes);
    let actual_sha = Sha1::from(&git_blob).digest().to_string();
    if actual_sha != expected_sha {
        return Err(WorkflowRunError::Malformed);
    }
    Ok(bytes)
}

fn digest_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn valid_git_sha(sha: &str) -> bool {
    sha.len() == 40
        && sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn get_json<T, R>(transport: &mut T, path: String, token: &str) -> Result<R, WorkflowRunError>
where
    T: Transport + ?Sized,
    R: for<'de> Deserialize<'de>,
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
        Err(TransportFail::Http(status)) if status >= 500 => {
            return Err(WorkflowRunError::Uncertain);
        }
        Err(TransportFail::Http(status)) => return Err(WorkflowRunError::HttpStatus(status)),
    };
    decode_json(&exchange)
}

fn decode_json<R>(exchange: &Exchange) -> Result<R, WorkflowRunError>
where
    R: for<'de> Deserialize<'de>,
{
    if exchange.status != 200 {
        return Err(if exchange.status >= 500 {
            WorkflowRunError::Uncertain
        } else {
            WorkflowRunError::HttpStatus(exchange.status)
        });
    }
    if exchange.body.len() > super::MAX_RESPONSE_BYTES {
        return Err(WorkflowRunError::ResponseTooLarge);
    }
    serde_json::from_slice(&exchange.body).map_err(|_| WorkflowRunError::Malformed)
}
