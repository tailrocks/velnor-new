//! Reviewed workflow and job authority for one repository connection.

mod host_config;
#[cfg(test)]
mod tests;
mod validate;

use std::collections::BTreeMap;

use crate::error::HostError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ConnectionId, PullRequestRule, WorkflowEvent};
pub(crate) use validate::validate_policies;

/// Authenticated facts required to find one policy row.
pub(crate) struct JobAuthorityLookup<'a> {
    pub(crate) connection_id: &'a ConnectionId,
    pub(crate) generation: u64,
    pub(crate) root_path: &'a str,
    pub(crate) workflow_path: &'a str,
    pub(crate) job_key: &'a str,
    pub(crate) event: WorkflowEvent,
    pub(crate) event_ref: &'a str,
    pub(crate) pull_request: Option<&'a PullRequestRule>,
}

const POLICY_SCHEMA: u32 = 1;
const MAX_POLICIES: usize = 32;
const MAX_WORKFLOWS: usize = 32;
const MAX_JOBS: usize = 512;
const MAX_CALLS: usize = 64;
const MAX_CONTEXTS: usize = 6;
const MAX_REFS: usize = 128;
const MAX_SECRETS: usize = 128;
const MAX_ENVIRONMENTS: usize = 32;
const MAX_NEEDS: usize = 256;
const MAX_TEXT_BYTES: usize = 512;
const MAX_POLICY_BYTES: usize = 256 * 1024;

/// One host-approved inventory for one repository connection.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApprovedJobAuthorityPolicy {
    schema_version: u32,
    connection_id: ConnectionId,
    binding_sha256: String,
    generation: u64,
    policy_sha256: String,
    generator: GeneratorIdentity,
    workflows: Vec<ApprovedWorkflow>,
}

/// Exact source identity for the generator that made this inventory.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GeneratorIdentity {
    source_sha: String,
    artifact_sha256: String,
}

/// One file in the authenticated workflow closure.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ApprovedWorkflow {
    repository_id: i64,
    path: String,
    content_sha256: String,
    calls: Vec<WorkflowCall>,
    jobs: Vec<ApprovedJobAuthorityEntry>,
}

/// One exact reusable workflow call edge.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct WorkflowCall {
    repository_id: i64,
    path: String,
    content_sha256: String,
}

/// One explicit authority rule for one leaf job key.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApprovedJobAuthorityEntry {
    root_path: String,
    workflow_path: String,
    job_key: String,
    role: JobAuthorityRole,
    contexts: Vec<ApprovedEventContext>,
    permissions_declared: bool,
    permissions: BTreeMap<String, PermissionLevel>,
    secrets: Vec<String>,
    environments: Vec<String>,
    condition_sha256: Option<String>,
    concurrency: Option<ConcurrencyControl>,
    needs: Vec<String>,
}

/// Authority granted to a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JobAuthorityRole {
    Verification,
    TrustedPublication,
}

/// One event and exact source context allowed for one job.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct ApprovedEventContext {
    event: WorkflowEvent,
    refs: Vec<String>,
    pull_request_rules: Vec<PullRequestRule>,
}

/// One upper bound for a GitHub token permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PermissionLevel {
    None,
    Read,
    Write,
}

/// Exact concurrency source recorded by the generator.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConcurrencyControl {
    scope: ConcurrencyScope,
    group: String,
    cancel_in_progress: String,
}

/// Location of a concurrency rule in the workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ConcurrencyScope {
    Workflow,
    Job,
}

impl ApprovedJobAuthorityPolicy {
    pub(crate) fn validate_for(
        &self,
        connection: &super::RepositoryConnection,
    ) -> Result<(), HostError> {
        validate::validate_policy(self, connection)
    }

    pub(crate) fn canonicalize(&mut self) {
        for workflow in &mut self.workflows {
            workflow.calls.sort();
            workflow.jobs.sort_by(|left, right| {
                (&left.root_path, &left.workflow_path, &left.job_key).cmp(&(
                    &right.root_path,
                    &right.workflow_path,
                    &right.job_key,
                ))
            });
            for job in &mut workflow.jobs {
                job.canonicalize();
            }
        }
        self.workflows.sort_by(|left, right| {
            (left.repository_id, &left.path).cmp(&(right.repository_id, &right.path))
        });
    }

    fn content(&self) -> PolicyContent<'_> {
        PolicyContent {
            schema_version: self.schema_version,
            connection_id: &self.connection_id,
            binding_sha256: &self.binding_sha256,
            generation: self.generation,
            generator: &self.generator,
            workflows: &self.workflows,
        }
    }

    pub(crate) fn connection_id(&self) -> &ConnectionId {
        &self.connection_id
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn binding_sha256(&self) -> &str {
        &self.binding_sha256
    }

    pub(crate) fn policy_sha256(&self) -> &str {
        &self.policy_sha256
    }

    pub(crate) fn canonical_json(&self) -> Result<Vec<u8>, HostError> {
        serde_json::to_vec(&self.content()).map_err(|_| HostError::Config)
    }

    pub(crate) fn workflow_content_sha256(&self, repository_id: i64, path: &str) -> Option<&str> {
        self.workflows
            .iter()
            .find(|workflow| workflow.repository_id == repository_id && workflow.path == path)
            .map(|workflow| workflow.content_sha256.as_str())
    }

    /// Read the immutable workflow and call inventory used to verify live source.
    pub(crate) fn workflow_sources(&self) -> impl Iterator<Item = ApprovedWorkflowSource<'_>> {
        self.workflows.iter().map(ApprovedWorkflowSource)
    }

    pub(crate) fn entry(
        &self,
        lookup: &JobAuthorityLookup<'_>,
    ) -> Option<&ApprovedJobAuthorityEntry> {
        self.workflows
            .iter()
            .flat_map(|workflow| &workflow.jobs)
            .find(|entry| {
                entry.root_path == lookup.root_path
                    && entry.workflow_path == lookup.workflow_path
                    && entry.job_key == lookup.job_key
                    && entry.contexts.iter().any(|context| context.matches(lookup))
            })
    }
}

/// Borrowed workflow identity and call edges from the validated authority inventory.
#[derive(Clone, Copy)]
pub(crate) struct ApprovedWorkflowSource<'a>(&'a ApprovedWorkflow);

impl<'a> ApprovedWorkflowSource<'a> {
    pub(crate) const fn repository_id(&self) -> i64 {
        self.0.repository_id
    }

    pub(crate) fn path(&self) -> &'a str {
        &self.0.path
    }

    pub(crate) fn content_sha256(&self) -> &'a str {
        &self.0.content_sha256
    }

    pub(crate) fn calls(&self) -> impl Iterator<Item = ApprovedWorkflowCall<'a>> + 'a {
        self.0.calls.iter().map(ApprovedWorkflowCall)
    }
}

/// Borrowed exact reusable-workflow edge from the approved call graph.
#[derive(Clone, Copy)]
pub(crate) struct ApprovedWorkflowCall<'a>(&'a WorkflowCall);

impl<'a> ApprovedWorkflowCall<'a> {
    pub(crate) const fn repository_id(&self) -> i64 {
        self.0.repository_id
    }

    pub(crate) fn path(&self) -> &'a str {
        &self.0.path
    }

    pub(crate) fn content_sha256(&self) -> &'a str {
        &self.0.content_sha256
    }
}

impl ApprovedJobAuthorityEntry {
    pub(crate) fn root_path(&self) -> &str {
        &self.root_path
    }

    pub(crate) fn workflow_path(&self) -> &str {
        &self.workflow_path
    }

    pub(crate) fn job_key(&self) -> &str {
        &self.job_key
    }

    pub(crate) const fn role(&self) -> JobAuthorityRole {
        self.role
    }

    pub(crate) fn canonical_json(&self) -> Result<Vec<u8>, HostError> {
        serde_json::to_vec(self).map_err(|_| HostError::Config)
    }

    fn canonicalize(&mut self) {
        for context in &mut self.contexts {
            context.refs.sort();
            context.pull_request_rules.sort();
        }
        self.contexts.sort();
        self.secrets.sort();
        self.environments.sort();
        self.needs.sort();
    }
}

impl ApprovedEventContext {
    fn matches(&self, lookup: &JobAuthorityLookup<'_>) -> bool {
        if self.event != lookup.event {
            return false;
        }
        if lookup.event == WorkflowEvent::PullRequest {
            return lookup.event_ref.is_empty()
                && lookup.pull_request.is_some_and(|actual| {
                    self.pull_request_rules.iter().any(|allowed| {
                        allowed.base_repository_id == actual.base_repository_id
                            && allowed.base_branch == actual.base_branch
                            && allowed.head_repository_id == actual.head_repository_id
                            && allowed.head_branch == actual.head_branch
                            && allowed
                                .number
                                .is_none_or(|number| actual.number == Some(number))
                    })
                });
        }
        self.refs.iter().any(|allowed| allowed == lookup.event_ref)
    }
}

#[derive(Serialize)]
struct PolicyContent<'a> {
    schema_version: u32,
    connection_id: &'a ConnectionId,
    binding_sha256: &'a str,
    generation: u64,
    generator: &'a GeneratorIdentity,
    workflows: &'a [ApprovedWorkflow],
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
