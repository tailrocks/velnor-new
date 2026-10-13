//! Stack-neutral GitHub Actions workflow IR.
use super::dispatch::WorkflowDispatch;
use super::jobs::is_safe_display_name;
use super::permissions::{PermissionLevel, Permissions};
pub use super::step::{Step, StepKind};
#[path = "job_validation.rs"]
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
    /// Validate push branch names, dispatch inputs, and schedule.
    fn validate(&self) -> Result<(), ContractError> {
        for branch in &self.push_branches {
            if !crate::is_valid_branch_name(branch) {
                return Err(ContractError::identity(
                    "trigger.push_branches",
                    format!("malformed_branch:{branch}"),
                ));
            }
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
/// Cron schedule for a generated workflow (P12-4 contract half).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleTrigger {
    /// Cron expressions (five fields each).
    pub cron: Vec<String>,
}

impl ScheduleTrigger {
    /// Validate five-field cron shape plus charset.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.cron.is_empty() {
            return Err(ContractError::identity("schedule.cron", "empty_cron"));
        }
        for entry in &self.cron {
            let fields: Vec<&str> = entry.split_whitespace().collect();
            let shape = fields.len() == 5
                && fields.iter().all(|field| {
                    !field.is_empty()
                        && field.bytes().all(|b| {
                            b.is_ascii_alphanumeric() || matches!(b, b'*' | b'/' | b'-' | b',')
                        })
                });
            if !shape {
                return Err(ContractError::identity(
                    "schedule.cron",
                    format!("bad_cron:{entry}"),
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
