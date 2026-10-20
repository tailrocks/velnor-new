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
    /// Optional workflow-wide GitHub Actions `cache-mode` policy (2026-09-10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_mode: Option<CacheMode>,
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
/// GitHub Actions `cache-mode` access control level (2026-09-10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheMode {
    /// Restore cache entries only (`read`).
    Read,
    /// Restore and save cache entries (`write`).
    Write,
    /// Save cache entries without restoring (`write-only`).
    WriteOnly,
    /// Disable all cache operations (`none`).
    None,
    /// Dynamic GitHub Actions expression (`${{ ... }}`), such as [`CACHE_MODE_PUSH_WRITE_EXPR`].
    #[serde(untagged)]
    Expression(String),
}
impl CacheMode {
    /// Canonical YAML string representation of this cache mode.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::WriteOnly => "write-only",
            Self::None => "none",
            Self::Expression(expr) => expr.as_str(),
        }
    }
    /// Push-only writer policy expression (`${{ github.event_name == 'push' && 'write' || 'read' }}`).
    #[must_use]
    pub fn push_write_expression() -> Self {
        Self::Expression(CACHE_MODE_PUSH_WRITE_EXPR.to_owned())
    }
    /// Parse a literal cache mode (`read`, `write`, `write-only`, `none`) or `${{ ... }}` expression.
    /// # Errors
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        let mode = match value {
            "read" => Self::Read,
            "write" => Self::Write,
            "write-only" => Self::WriteOnly,
            "none" => Self::None,
            expr if expr.starts_with("${{") && expr.ends_with("}}") => {
                Self::Expression(expr.to_owned())
            }
            _ => {
                return Err(ContractError::identity(
                    "cache_mode",
                    format!("invalid_cache_mode:{value}"),
                ));
            }
        };
        mode.validate("cache_mode")?;
        Ok(mode)
    }
    /// Validate that an expression cache mode is a single-line `${{ ... }}` expression.
    /// # Errors
    pub fn validate(&self, field: &'static str) -> Result<(), ContractError> {
        if let Self::Expression(expr) = self {
            let inner = expr
                .strip_prefix("${{")
                .and_then(|rest| rest.strip_suffix("}}"))
                .map(str::trim);
            if inner.is_none_or(str::is_empty) || expr.chars().any(char::is_control) {
                return Err(ContractError::identity(field, "bad_cache_mode_expression"));
            }
        }
        Ok(())
    }
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
    /// Optional per-job GitHub Actions `cache-mode` override (2026-09-10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_mode: Option<CacheMode>,
    /// Optional protected environment bound to this job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    /// Catalog service containers started beside the job.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<super::services::ServiceContainer>,
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
        if let Some(cache_mode) = &self.cache_mode {
            cache_mode.validate("workflow.cache_mode")?;
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
    /// Validate pull-request types, push branch names, dispatch inputs, and schedule.
    fn validate(&self) -> Result<(), ContractError> {
        for pr_type in &self.pull_request_types {
            if super::workflow_policy::is_restricted_action_event(pr_type) {
                return Err(ContractError::identity(
                    "trigger.pull_request_types",
                    format!("restricted_untrusted_actor_event:{pr_type}"),
                ));
            }
        }
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
    /// Construct a schedule trigger from five-field cron expressions.
    #[must_use]
    pub fn new(cron: Vec<String>) -> Self {
        Self { cron }
    }

    /// Construct a schedule trigger with a single five-field cron expression.
    #[must_use]
    pub fn single(expr: impl Into<String>) -> Self {
        Self {
            cron: vec![expr.into()],
        }
    }

    /// Validate five-field cron shape, charset, and per-field numeric bounds.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.cron.is_empty() {
            return Err(ContractError::identity("schedule.cron", "empty_cron"));
        }
        let bounds: [(u32, u32); 5] = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)];
        for entry in &self.cron {
            let fields: Vec<&str> = entry.split_whitespace().collect();
            let valid = fields.len() == 5
                && fields
                    .into_iter()
                    .zip(bounds)
                    .all(|(field, (min, max))| is_valid_cron_field(field, min, max));
            if !valid {
                return Err(ContractError::identity(
                    "schedule.cron",
                    format!("bad_cron:{entry}"),
                ));
            }
        }
        Ok(())
    }
}

fn is_valid_cron_field(field: &str, min: u32, max: u32) -> bool {
    !field.is_empty()
        && field
            .split(',')
            .all(|part| is_valid_cron_part(part, min, max))
}

fn is_valid_cron_part(part: &str, min: u32, max: u32) -> bool {
    if part.is_empty() {
        return false;
    }
    let (range_part, step_ok) = match part.split_once('/') {
        Some((base, step)) => (
            base,
            parse_cron_num(step).is_some_and(|s| s >= 1 && s <= max.max(1)),
        ),
        None => (part, true),
    };
    if !step_ok || range_part.is_empty() {
        return false;
    }
    if range_part == "*" {
        return true;
    }
    if let Some((start_str, end_str)) = range_part.split_once('-') {
        let Some(start) = parse_cron_num(start_str) else {
            return false;
        };
        let Some(end) = parse_cron_num(end_str) else {
            return false;
        };
        start >= min && end <= max && start <= end
    } else {
        parse_cron_num(range_part).is_some_and(|n| n >= min && n <= max)
    }
}

fn parse_cron_num(text: &str) -> Option<u32> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse::<u32>().ok())
        .flatten()
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
