//! Canonical nonsecret source values stored with an assigned request.

use serde::{Deserialize, Serialize};

use crate::error::HostError;
use crate::launch::steps::source::AuthorizedSourceReceipt;
use velnor_runner_github::workflow_run::{VerifiedWorkflowSource, WorkflowRefKind};

#[derive(Clone, Copy, Serialize)]
pub(in crate::journal) struct EventSource<'a> {
    pub(in crate::journal) owner_name: Option<&'a str>,
    pub(in crate::journal) repository_name: Option<&'a str>,
    pub(in crate::journal) job_workflow_ref: Option<&'a str>,
    pub(in crate::journal) workflow_run_id: Option<i64>,
    pub(in crate::journal) event_name: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredSource {
    repository_id: i64,
    owner_name: String,
    repository_name: String,
    workflow_run_id: i64,
    event_name: String,
    head_branch: Option<String>,
    head_sha: String,
    job_workflow_ref: String,
    workflow: StoredWorkflow,
    workflow_source_sha: Option<String>,
    run_attempt: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredWorkflow {
    full_ref: String,
    owner_name: String,
    repository_name: String,
    workflow_path: String,
    reference: String,
    kind: String,
}

impl StoredSource {
    pub(super) fn from_receipt(
        receipt: &AuthorizedSourceReceipt,
        expected_repository_id: i64,
    ) -> Result<Self, HostError> {
        let value = Self::from_verified(receipt.source());
        value.validate(expected_repository_id)?;
        Ok(value)
    }

    pub(super) fn parse(value: &str, expected_repository_id: i64) -> Result<Self, HostError> {
        let stored: Self = serde_json::from_str(value).map_err(|_| HostError::Journal)?;
        if stored.json()? != value {
            return Err(HostError::Journal);
        }
        stored.validate(expected_repository_id)?;
        Ok(stored)
    }

    pub(super) fn json(&self) -> Result<String, HostError> {
        serde_json::to_string(self).map_err(|_| HostError::Journal)
    }

    pub(super) fn matches_event_source(&self, event: EventSource<'_>) -> bool {
        event
            .owner_name
            .is_none_or(|value| self.owner_name == value)
            && event
                .repository_name
                .is_none_or(|value| self.repository_name == value)
            && event
                .job_workflow_ref
                .is_none_or(|value| self.job_workflow_ref == value)
            && event
                .workflow_run_id
                .is_none_or(|value| self.workflow_run_id == value)
            && event
                .event_name
                .is_none_or(|value| self.event_name == value)
    }

    fn from_verified(source: &VerifiedWorkflowSource) -> Self {
        Self {
            repository_id: source.repository_id,
            owner_name: source.owner_name.clone(),
            repository_name: source.repository_name.clone(),
            workflow_run_id: source.workflow_run_id,
            event_name: source.event_name.clone(),
            head_branch: source.head_branch.clone(),
            head_sha: source.head_sha.clone(),
            job_workflow_ref: source.job_workflow_ref.clone(),
            workflow: StoredWorkflow {
                full_ref: source.job_workflow.full_ref.clone(),
                owner_name: source.job_workflow.owner_name.clone(),
                repository_name: source.job_workflow.repository_name.clone(),
                workflow_path: source.job_workflow.workflow_path.clone(),
                reference: source.job_workflow.reference.clone(),
                kind: kind_name(source.job_workflow.kind).to_owned(),
            },
            workflow_source_sha: source.workflow_source_sha.clone(),
            run_attempt: source.run_attempt,
        }
    }

    fn validate(&self, expected_repository_id: i64) -> Result<(), HostError> {
        if self.repository_id != expected_repository_id
            || self.repository_id <= 0
            || self.workflow_run_id <= 0
            || self.run_attempt <= 0
            || !valid_text(&self.owner_name, 256)
            || !valid_text(&self.repository_name, 256)
            || !valid_text(&self.event_name, 128)
            || self
                .head_branch
                .as_deref()
                .is_some_and(|value| !valid_text(value, 1024))
            || !valid_sha(&self.head_sha)
            || !valid_text(&self.job_workflow_ref, 4096)
            || self.job_workflow_ref != self.workflow.full_ref
            || !valid_text(&self.workflow.owner_name, 256)
            || !valid_text(&self.workflow.repository_name, 256)
            || !valid_text(&self.workflow.workflow_path, 2048)
            || !valid_text(&self.workflow.reference, 1024)
            || !matches!(
                self.workflow.kind.as_str(),
                "branch" | "tag" | "pull_request" | "merge_queue" | "commit" | "other"
            )
            || self
                .workflow_source_sha
                .as_deref()
                .is_some_and(|value| !valid_sha(value))
        {
            return Err(HostError::Identity);
        }
        Ok(())
    }
}

fn kind_name(kind: WorkflowRefKind) -> &'static str {
    match kind {
        WorkflowRefKind::Branch => "branch",
        WorkflowRefKind::Tag => "tag",
        WorkflowRefKind::PullRequest => "pull_request",
        WorkflowRefKind::MergeQueue => "merge_queue",
        WorkflowRefKind::Commit => "commit",
        WorkflowRefKind::Other => "other",
    }
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
