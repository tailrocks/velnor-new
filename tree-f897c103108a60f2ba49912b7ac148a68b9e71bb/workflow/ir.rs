//! Stack-neutral GitHub Actions workflow IR.
use super::jobs::ScheduleTrigger;
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

/// One GitHub token permission scope level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    /// Read-only access.
    Read,
    /// Read-write access.
    Write,
    /// No access.
    None,
}

/// Workflow or job permissions (typed scopes; renderer emits YAML).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions {
    /// Repository contents scope.
    pub contents: PermissionLevel,
    /// Pull-requests scope.
    pub pull_requests: PermissionLevel,
    /// OIDC token scope (trusted publishing only).
    pub id_token: PermissionLevel,
    /// Actions scope.
    pub actions: PermissionLevel,
}

impl Permissions {
    /// True when every scope is `write` (always rejected).
    #[must_use]
    pub fn is_write_all(&self) -> bool {
        [
            self.contents,
            self.pull_requests,
            self.id_token,
            self.actions,
        ]
        .iter()
        .all(|level| matches!(level, PermissionLevel::Write))
    }
}

impl Default for Permissions {
    /// CI default: `contents`/`actions` read, everything else none.
    fn default() -> Self {
        Self {
            contents: PermissionLevel::Read,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            actions: PermissionLevel::Read,
        }
    }
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
    /// Literal versioned Ubuntu label.
    pub runs_on: String,
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
/// One workflow step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Step name.
    pub name: String,
    /// Step payload.
    #[serde(flatten)]
    pub kind: StepKind,
}
/// Step payload variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepKind {
    /// Pinned GitHub Action step.
    Action {
        /// Full-SHA `uses` reference.
        uses: String,
        /// Action inputs.
        #[serde(default)]
        with: BTreeMap<String, String>,
    },
    /// Fixed shell argv step.
    Shell {
        /// Fixed argument vector.
        run: Vec<String>,
        /// Fixed environment.
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// Fixed internal planner/aggregation step.
    Internal {
        /// Internal operation name.
        operation: String,
    },
}
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
        if !is_pinned_label(&self.runs_on) {
            return Err(ContractError::identity(
                "job.runs_on",
                format!("unpinned_label:{id}"),
            ));
        }
        for need in &self.needs {
            if !ids.contains(need.as_str()) {
                return Err(ContractError::identity(
                    "job.needs",
                    format!("unknown_job:{need}"),
                ));
            }
        }
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
        if matches!(effective.id_token, PermissionLevel::Write) && self.environment.is_none() {
            return Err(ContractError::identity(
                "job.environment",
                format!("id_token_write_needs_environment:{id}"),
            ));
        }
        if pr_triggered && matches!(effective.contents, PermissionLevel::Write) {
            return Err(ContractError::identity(
                "job.permissions",
                format!("contents_write_on_pr:{id}"),
            ));
        }
        if self.steps.is_empty() {
            return Err(ContractError::identity(
                "job.steps",
                format!("empty_steps:{id}"),
            ));
        }
        for step in &self.steps {
            step.validate(id)?;
        }
        Ok(())
    }
}
impl Step {
    /// Validate one step payload.
    fn validate(&self, job: &str) -> Result<(), ContractError> {
        if self.name.trim().is_empty() {
            return Err(ContractError::identity(
                "step.name",
                format!("empty_name:{job}"),
            ));
        }
        match &self.kind {
            StepKind::Action { uses, .. } => {
                if uses.trim().is_empty() {
                    return Err(ContractError::identity(
                        "step.uses",
                        format!("empty_uses:{job}"),
                    ));
                }
            }
            StepKind::Shell { run, .. } => {
                if run.is_empty() || run.iter().any(|arg| arg.trim().is_empty()) {
                    return Err(ContractError::identity(
                        "step.run",
                        format!("bad_argv:{job}"),
                    ));
                }
            }
            StepKind::Internal { operation } => {
                if operation.trim().is_empty() {
                    return Err(ContractError::identity(
                        "step.operation",
                        format!("empty_operation:{job}"),
                    ));
                }
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

/// Check for a literal versioned label (no `latest` aliases or expressions).
fn is_pinned_label(label: &str) -> bool {
    !label.is_empty()
        && !label.contains("${{")
        && !label.contains("latest")
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
}
