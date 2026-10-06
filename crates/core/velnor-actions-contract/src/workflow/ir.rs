//! Stack-neutral GitHub Actions workflow IR.
use super::jobs::{ScheduleTrigger, is_safe_display_name};
use super::permissions::{PermissionLevel, Permissions};
pub use super::step::{Step, StepKind};
mod job_validation;
use super::timeout::JobTimeout;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
/// Stack-neutral GitHub Actions workflow IR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowIr {
    /// Workflow display name.
    pub name: String,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trigger {
    /// Pull-request event types.
    pub pull_request_types: Vec<String>,
    /// Push branches (exactly the default branch).
    pub push_branches: Vec<String>,
    /// Whether `merge_group` is enabled.
    pub merge_group: bool,
    /// Optional `workflow_dispatch` inputs (exact-plan bootstrap dispatch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_dispatch: Option<WorkflowDispatch>,
    /// Optional cron schedule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<ScheduleTrigger>,
}
/// Typed `workflow_dispatch` inputs (exact-plan references only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDispatch {
    /// Dispatch inputs, sorted by name, unique.
    pub inputs: Vec<DispatchInput>,
}
/// One typed dispatch input (always rendered as `type: string`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchInput {
    /// Input name (`[a-z0-9-_]`).
    pub name: String,
    /// Whether the dispatcher must supply a value.
    pub required: bool,
    /// Optional default value (ASCII, no control characters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

impl DispatchInput {
    /// Rendered input type: always `string`, never boolean/choice.
    pub const INPUT_TYPE: &'static str = "string";
}

/// Concurrency group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Concurrency {
    /// Concurrency group expression.
    pub group: String,
    /// Cancel-in-progress expression.
    pub cancel_in_progress: String,
}
/// One workflow job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Stable display name.
    pub display_name: String,
    /// Literal hosted or typed ephemeral runner label.
    pub runs_on: String,
    /// Explicit platform and capability policy for repository-owned check jobs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_runner: Option<crate::config::CheckRunner>,
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
    /// Ordered steps.
    pub steps: Vec<Step>,
}
/// Runtime gate for cache saves: producer success on pushes (trusted scope).
///
/// A step-level `if:` REPLACES GitHub's default `success()`, so the gate
/// must restate it: without `success()`, the save step would run after a
/// failed producer and poison the trusted layer with failed output.
/// `pull_request` runs — same-repo or fork — restore read-only: the pinned
/// cache actions have no PR-scoped save support, so a PR save could never
/// promote safely. Push runs save into the repository that owns the run
/// (GitHub cache scope is per-repo), keeping fork pushes confined to the
/// fork. The Mise adapter's trust predicates implement this same policy
/// over runtime values; this string is its generation-time spelling.
pub const CACHE_SAVE_CONDITION: &str = "success() && github.event_name == 'push'";
/// `env:` spelling of the push-only writer policy for cache-mode inputs.
///
/// Evaluates to `write` on push runs and `read` everywhere else, so a
/// cache action that saves from its post step restores on every event
/// but only ever writes on push (same policy as
/// [`CACHE_SAVE_CONDITION`], in the value position the mode supports).
pub const CACHE_MODE_PUSH_WRITE_EXPR: &str =
    "${{ github.event_name == 'push' && 'write' || 'read' }}";

impl WorkflowIr {
    /// Validate names, permissions, triggers, jobs, and step payloads.
    ///
    /// Effective permissions are per job (override or workflow level):
    /// `id-token: write` requires a job environment, `contents: write` is
    /// forbidden in pull-request-triggered workflows, and write-all is
    /// rejected everywhere.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.name.trim().is_empty() {
            return Err(ContractError::identity("workflow.name", "empty_name"));
        }
        if self.permissions.is_write_all() {
            return Err(ContractError::identity("workflow.permissions", "write_all"));
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
        }
        Ok(())
    }
}
impl Trigger {
    /// Validate dispatch inputs and schedule (other fields pass through).
    fn validate(&self) -> Result<(), ContractError> {
        if let Some(dispatch) = &self.workflow_dispatch {
            dispatch.validate()?;
        }
        if let Some(schedule) = &self.schedule {
            schedule.validate()?;
        }
        Ok(())
    }
}
impl WorkflowDispatch {
    /// Validate input names (charset, sorted, unique) and defaults.
    fn validate(&self) -> Result<(), ContractError> {
        let names: Vec<&str> = self
            .inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        if sorted != names {
            return Err(ContractError::identity(
                "trigger.dispatch.inputs",
                "must_be_sorted",
            ));
        }
        let unique: BTreeSet<&str> = names.iter().copied().collect();
        if unique.len() != names.len() {
            return Err(ContractError::identity(
                "trigger.dispatch.inputs",
                "duplicate_input",
            ));
        }
        for input in &self.inputs {
            input.validate()?;
        }
        Ok(())
    }
}
impl DispatchInput {
    /// Validate name charset and default value safety.
    fn validate(&self) -> Result<(), ContractError> {
        let name = self.name.as_str();
        let charset = name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'));
        if name.is_empty() || !charset {
            return Err(ContractError::identity(
                "trigger.dispatch.inputs.name",
                format!("bad_name:{name}"),
            ));
        }
        if let Some(default) = &self.default {
            let safe =
                !default.is_empty() && default.bytes().all(|b| b.is_ascii_graphic() || b == b' ');
            if !safe {
                return Err(ContractError::identity(
                    "trigger.dispatch.inputs.default",
                    format!("bad_default:{name}"),
                ));
            }
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

/// Check for a hosted label or a typed scale-set IR token.
fn is_pinned_label(label: &str) -> bool {
    crate::config::is_legacy_hosted_label(label)
        || crate::config::ScaleSetSelector::parse_token(label).is_ok()
}
