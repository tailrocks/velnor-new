//! Authenticated workflow source facts bound to one hook request.

use serde::{Deserialize, Serialize};
use std::ops::Deref;

use super::ExecutionSourceRequest;

#[cfg(test)]
mod tests;
mod validation;
mod verifier;

pub(crate) use verifier::verify_live_source;

/// One exact file in the authenticated workflow source closure.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerifiedWorkflowIdentity {
    repository_id: u64,
    path: String,
    commit_sha: String,
    tree_sha: String,
    blob_sha: String,
    content_sha256: String,
}

impl VerifiedWorkflowIdentity {
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn content_sha256(&self) -> &str {
        &self.content_sha256
    }

    fn valid(&self) -> bool {
        self.repository_id > 0
            && super::valid_relative_path(&self.path)
            && super::valid_git_sha(&self.commit_sha)
            && super::valid_git_sha(&self.tree_sha)
            && super::valid_git_sha(&self.blob_sha)
            && super::valid_digest(&self.content_sha256)
    }
}

/// Deserialized journal data. This type is untrusted persisted evidence, not
/// proof that the source verifier accepted the workflow.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedExecutionSource {
    request: ExecutionSourceRequest,
    request_digest: String,
    tested_commit_sha: String,
    root_workflow_path: String,
    root_commit_sha: String,
    root_tree_sha: String,
    root_blob_sha: String,
    root_content_sha256: String,
    leaf_workflow_path: String,
    leaf_commit_sha: String,
    leaf_tree_sha: String,
    leaf_blob_sha: String,
    leaf_content_sha256: String,
    workflow_closure: Vec<VerifiedWorkflowIdentity>,
    pull_request_source: Option<PersistedPullRequestSource>,
    rest_job_id: Option<String>,
}

/// GitHub API facts for the pull request whose merge commit the Runner tested.
///
/// This record is untrusted after journal deserialization. A source verifier
/// must obtain these values from the selected repository's immutable API
/// response before it can construct a live `AuthorizedExecutionSource`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedPullRequestSource {
    number: u64,
    base_repository_id: i64,
    base_repository_name: String,
    base_ref: String,
    base_sha: String,
    head_repository_id: i64,
    head_repository_name: String,
    head_ref: String,
    head_sha: String,
    merge_ref: String,
    merge_commit_sha: String,
}

/// Source facts accepted by the source verifier for one actual job.
///
/// This wrapper has no deserializer. Only the source-verifier module can make
/// one, after it has checked the API response and the complete workflow tree.
pub(crate) struct AuthorizedExecutionSource {
    record: PersistedExecutionSource,
    channel_digest: String,
}

impl Deref for AuthorizedExecutionSource {
    type Target = PersistedExecutionSource;

    fn deref(&self) -> &Self::Target {
        &self.record
    }
}

impl AuthorizedExecutionSource {
    pub(crate) fn validate(&self) -> Result<(), crate::error::HostError> {
        self.record.validate()?;
        if !super::valid_digest(&self.channel_digest) {
            return Err(crate::error::HostError::Identity);
        }
        Ok(())
    }

    pub(crate) fn channel_digest(&self) -> &str {
        &self.channel_digest
    }
}

#[cfg(test)]
pub(crate) fn test_authorized_execution_source(
    record: PersistedExecutionSource,
    channel_digest: String,
) -> Result<AuthorizedExecutionSource, crate::error::HostError> {
    let source = AuthorizedExecutionSource {
        record,
        channel_digest,
    };
    source.validate()?;
    Ok(source)
}
