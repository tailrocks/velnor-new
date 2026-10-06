//! Stack-neutral GitHub Actions workflow IR.
use super::jobs::{ScheduleTrigger, is_safe_display_name};
use super::permissions::{PermissionLevel, Permissions};
pub use super::step::{Step, StepKind};
use super::timeout::JobTimeout;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
#[path = "mbx_role_tests.rs"]
mod mbx_role_tests;
#[path = "job_producer_validation.rs"]
mod producer_validation;
/// Stack-neutral GitHub Actions workflow IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowIr {
    /// Mandatory literal cache-service default; generated workflows require read.
    pub cache_mode: super::cache_mode::CacheMode,
    /// Workflow display name.
    pub name: String,
    /// Optional workflow run display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_name: Option<String>,
    /// Event triggers.
    pub triggers: Trigger,
    /// Workflow permissions.
    pub permissions: Permissions,
    /// Concurrency group.
    pub concurrency: Concurrency,
    /// Jobs keyed by job ID (sorted).
    pub jobs: BTreeMap<String, Job>,
}
/// Event triggers for generated workflows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    /// Pull-request event types.
    pub pull_request_types: Vec<String>,
    /// Push branches (exactly the default branch).
    pub push_branches: Vec<String>,
    /// Closed supported tag patterns (currently `v*`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub push_tags: Vec<String>,
    /// Whether `merge_group` is enabled.
    pub merge_group: bool,
    /// Optional typed manual-dispatch inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_dispatch: Option<WorkflowDispatch>,
    /// Optional cron schedule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<ScheduleTrigger>,
}
pub use super::dispatch::{DispatchInput, DispatchInputType, WorkflowDispatch};

/// Concurrency group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Concurrency {
    /// Concurrency group expression.
    pub group: String,
    /// Cancel-in-progress expression.
    pub cancel_in_progress: String,
}
/// One workflow job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    /// Literal cache-service override; absent jobs inherit the workflow read mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_mode: Option<super::cache_mode::CacheMode>,
    /// Stable display name.
    pub display_name: String,
    /// Literal versioned Ubuntu label.
    pub runs_on: String,
    /// Per-job timeout (required: no job inherits the 6 h default).
    pub timeout_minutes: JobTimeout,
    /// Job dependencies.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Run condition (`if`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Optional per-job permission override (else workflow permissions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Permissions>,
    /// Optional protected environment bound to this job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    /// Closed isolated source-producing role and evidence bindings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_producer: Option<super::source_producer::SourceProducer>,
    /// Closed isolated complete tool payload producer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_producer: Option<super::tool_producer::PureToolProducer>,
    /// Closed isolated publication of authenticated native MBX data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mbx_producer: Option<super::mbx_producer::PureMbxProducer>,
    /// Closed native Pages deployment authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_pages_deploy: Option<super::pages::NativePagesDeploy>,
    /// Closed public GitHub attestation authority; generation approval is separate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_publish: Option<super::native_publish::NativePublishRole>,
    /// Closed references to outputs from declared action and native helper steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<super::outputs::JobOutput>,
    /// Ordered steps.
    pub steps: Vec<Step>,
}
pub use super::cache_trust::{
    CACHE_DEFAULT_BRANCH_WRITE_EXPR, CACHE_MODE_PUSH_WRITE_EXPR, CACHE_MODE_PUSH_WRITE_INNER,
    CACHE_SAVE_CONDITION, CACHE_TRUSTED_PUSH_EXPR,
};

impl WorkflowIr {
    /// Validate names, permissions, triggers, jobs, and step payloads.
    ///
    /// Effective permissions are per job (override or workflow level):
    /// `id-token: write` requires an approved closed role and environment; `contents: write` is
    /// forbidden in pull-request-triggered workflows, and write-all is
    /// rejected everywhere.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        super::cache_mode::validate_workflow(self)?;
        if self.name.trim().is_empty() {
            return Err(ContractError::identity("workflow.name", "empty_name"));
        }
        if self
            .run_name
            .as_ref()
            .is_some_and(|name| name.trim().is_empty() || name.chars().any(char::is_control))
        {
            return Err(ContractError::identity(
                "workflow.run_name",
                "invalid_run_name",
            ));
        }
        if self.permissions.is_write_all() {
            return Err(ContractError::identity("workflow.permissions", "write_all"));
        }
        if matches!(self.permissions.issues, PermissionLevel::Write) {
            return Err(ContractError::identity(
                "workflow.permissions",
                "issues_write_needs_observer_override",
            ));
        }
        if matches!(self.permissions.pages, PermissionLevel::Write) {
            return Err(ContractError::identity(
                "workflow.permissions",
                "pages_write_needs_native_deploy_override",
            ));
        }
        if matches!(self.permissions.attestations, PermissionLevel::Write) {
            return Err(ContractError::identity(
                "workflow.permissions",
                "attestations_write_needs_closed_override",
            ));
        }
        self.triggers.validate()?;
        if self.concurrency.group.trim().is_empty() {
            return Err(ContractError::identity(
                "workflow.concurrency",
                "empty_group",
            ));
        }
        if self.jobs.is_empty() {
            return Err(ContractError::identity("workflow.jobs", "empty_jobs"));
        }
        let ids: BTreeSet<&str> = self.jobs.keys().map(String::as_str).collect();
        let pr_triggered = !self.triggers.pull_request_types.is_empty();
        for (id, job) in &self.jobs {
            job.validate(id, &ids, &self.permissions, pr_triggered)?;
            super::observer::validate_observer(id, job, &self.permissions, &self.triggers)?;
            if let Some(publish) = &job.native_publish {
                publish.validate(job, self)?;
            }
            if let Some(deploy) = &job.native_pages_deploy {
                deploy.validate(job, self)?;
            }
        }
        Ok(())
    }
}
impl Trigger {
    /// Validate dispatch inputs and schedule (other fields pass through).
    fn validate(&self) -> Result<(), ContractError> {
        if self.push_tags.len() > 1
            || self
                .push_tags
                .iter()
                .any(|pattern| !matches!(pattern.as_str(), "v*" | "v[0-9]*"))
        {
            return Err(ContractError::identity(
                "trigger.push_tags",
                "unsupported_tag_patterns",
            ));
        }
        if let Some(dispatch) = &self.workflow_dispatch {
            dispatch.validate()?;
        }
        if let Some(schedule) = &self.schedule {
            schedule.validate()?;
        }
        Ok(())
    }
}
impl Job {
    /// Validate one job: labels, refs, effective permissions, steps.
    fn validate(
        &self,
        id: &str,
        ids: &BTreeSet<&str>,
        workflow: &Permissions,
        pr_triggered: bool,
    ) -> Result<(), ContractError> {
        if self.display_name.trim().is_empty() {
            return Err(ContractError::identity("job.display_name", "empty_name"));
        }
        if !is_safe_display_name(&self.display_name) {
            return Err(ContractError::identity(
                "job.display_name",
                format!("bad_display_name:{id}"),
            ));
        }
        if !is_pinned_label(&self.runs_on) {
            return Err(ContractError::identity(
                "job.runs_on",
                format!("unpinned_label:{id}"),
            ));
        }
        self.timeout_minutes.validate()?;
        for need in &self.needs {
            if !ids.contains(need.as_str()) {
                return Err(ContractError::identity(
                    "job.needs",
                    format!("unknown_job:{need}"),
                ));
            }
        }
        self.validate_permissions(id, workflow, pr_triggered)?;
        if self.steps.is_empty() {
            return Err(ContractError::identity(
                "job.steps",
                format!("empty_steps:{id}"),
            ));
        }
        self.validate_producers()?;
        super::step::validate_step_ids(&self.steps)?;
        super::outputs::validate_job_outputs(&self.outputs, &self.steps)?;
        for step in &self.steps {
            step.validate(id)?;
        }
        Ok(())
    }
}
impl Job {
    fn validate_producers(&self) -> Result<(), ContractError> {
        producer_validation::validate(self)
    }

    fn validate_permissions(
        &self,
        id: &str,
        workflow: &Permissions,
        pr_triggered: bool,
    ) -> Result<(), ContractError> {
        if self
            .permissions
            .as_ref()
            .is_some_and(Permissions::is_write_all)
        {
            return Err(ContractError::identity(
                "job.permissions",
                format!("write_all:{id}"),
            ));
        }
        if let Some(environment) = &self.environment
            && !is_valid_environment(environment)
        {
            return Err(ContractError::identity(
                "job.environment",
                format!("bad_environment:{id}"),
            ));
        }
        let effective = self.permissions.as_ref().unwrap_or(workflow);
        if matches!(effective.pages, PermissionLevel::Write) && self.native_pages_deploy.is_none() {
            return Err(ContractError::identity(
                "job.permissions",
                format!("pages_write_needs_native_deploy_role:{id}"),
            ));
        }
        if matches!(effective.id_token, PermissionLevel::Write)
            && self.environment.is_none()
            && self.native_publish.is_none()
        {
            return Err(ContractError::identity(
                "job.environment",
                format!("id_token_write_needs_environment:{id}"),
            ));
        }
        if matches!(effective.id_token, PermissionLevel::Write)
            && self.native_pages_deploy.is_none()
            && self.native_publish.is_none()
        {
            return Err(ContractError::identity(
                "job.permissions",
                format!("id_token_write_needs_closed_role:{id}"),
            ));
        }
        if matches!(effective.attestations, PermissionLevel::Write) && self.native_publish.is_none()
        {
            return Err(ContractError::identity(
                "job.permissions",
                format!("attestations_write_needs_closed_role:{id}"),
            ));
        }
        if pr_triggered && matches!(effective.contents, PermissionLevel::Write) {
            return Err(ContractError::identity(
                "job.permissions",
                format!("contents_write_on_pr:{id}"),
            ));
        }
        Ok(())
    }
}
/// Check a protected environment name (nonempty, safe charset/segments).
fn is_valid_environment(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'/' | b'.'))
        && !name.split('/').any(|seg| seg.is_empty() || seg == "..")
}

/// Check for a literal versioned label (no `latest` aliases or expressions).
fn is_pinned_label(label: &str) -> bool {
    !label.is_empty()
        && !label.contains("${{")
        && !label.contains("latest")
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
}
