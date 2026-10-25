//! Canonical hook request from the fixed Runner pre-job hook.

use serde::{Deserialize, Serialize};

use crate::error::HostError;

use super::{
    MAX_REQUEST_BYTES, digest, optional_git_sha, optional_text, valid_digest, valid_git_sha,
    valid_text,
};

/// Untrusted, allowlisted Runner values from the fixed pre-job hook.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionSourceRequest {
    schema_version: u32,
    challenge: String,
    reservation_id: i64,
    intent_id: i64,
    repository_id: u64,
    repository_name: String,
    event_name: String,
    event_ref: String,
    tested_sha: String,
    run_id: u64,
    run_attempt: u32,
    job_key: String,
    workflow_ref: String,
    workflow_sha: String,
    base_ref: Option<String>,
    base_sha: Option<String>,
    head_ref: Option<String>,
    head_sha: Option<String>,
    pull_request_number: Option<u64>,
}

impl ExecutionSourceRequest {
    /// Parse one compact canonical request under the fixed size limit.
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, HostError> {
        if bytes.is_empty() || bytes.len() > MAX_REQUEST_BYTES {
            return Err(HostError::Identity);
        }
        let request: Self = serde_json::from_slice(bytes).map_err(|_| HostError::Identity)?;
        if request.canonical_json()?.as_slice() != bytes || !request.valid() {
            return Err(HostError::Identity);
        }
        Ok(request)
    }

    pub(crate) fn canonical_json(&self) -> Result<Vec<u8>, HostError> {
        serde_json::to_vec(self).map_err(|_| HostError::Identity)
    }

    pub(crate) fn sha256(&self) -> Result<String, HostError> {
        Ok(digest(&self.canonical_json()?))
    }

    pub(crate) fn challenge(&self) -> &str {
        &self.challenge
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn intent_id(&self) -> i64 {
        self.intent_id
    }

    pub(crate) const fn repository_id(&self) -> u64 {
        self.repository_id
    }

    pub(crate) fn repository_name(&self) -> &str {
        &self.repository_name
    }

    pub(crate) fn event_name(&self) -> &str {
        &self.event_name
    }

    pub(crate) fn event_ref(&self) -> &str {
        &self.event_ref
    }

    pub(crate) fn tested_sha(&self) -> &str {
        &self.tested_sha
    }

    pub(crate) const fn run_id(&self) -> u64 {
        self.run_id
    }

    pub(crate) const fn run_attempt(&self) -> u32 {
        self.run_attempt
    }

    pub(crate) fn job_key(&self) -> &str {
        &self.job_key
    }

    pub(crate) fn workflow_ref(&self) -> &str {
        &self.workflow_ref
    }

    pub(crate) fn workflow_sha(&self) -> &str {
        &self.workflow_sha
    }

    pub(crate) fn base_ref(&self) -> Option<&str> {
        self.base_ref.as_deref()
    }

    pub(crate) fn base_sha(&self) -> Option<&str> {
        self.base_sha.as_deref()
    }

    pub(crate) fn head_ref(&self) -> Option<&str> {
        self.head_ref.as_deref()
    }

    pub(crate) fn head_sha(&self) -> Option<&str> {
        self.head_sha.as_deref()
    }

    pub(crate) const fn pull_request_number(&self) -> Option<u64> {
        self.pull_request_number
    }

    pub(crate) fn valid(&self) -> bool {
        self.schema_version == 1
            && valid_digest(&self.challenge)
            && self.reservation_id > 0
            && self.intent_id > 0
            && self.repository_id > 0
            && valid_text(&self.repository_name, 512)
            && valid_text(&self.event_name, 64)
            && valid_text(&self.event_ref, 1024)
            && valid_git_sha(&self.tested_sha)
            && self.run_id > 0
            && self.run_attempt > 0
            && valid_text(&self.job_key, 128)
            && valid_text(&self.workflow_ref, 2048)
            && valid_git_sha(&self.workflow_sha)
            && optional_text(self.base_ref.as_deref(), 1024)
            && optional_git_sha(self.base_sha.as_deref())
            && optional_text(self.head_ref.as_deref(), 1024)
            && optional_git_sha(self.head_sha.as_deref())
            && self.pull_request_number.is_none_or(|number| number > 0)
    }
}
