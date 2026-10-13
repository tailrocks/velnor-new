//! Stack-neutral GitHub Actions workflow IR.
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
}
/// Workflow permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permissions {
    /// Contents permission (`read`).
    pub contents: String,
    /// Actions permission (`read`).
    pub actions: String,
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
    /// Ordered steps.
    pub steps: Vec<Step>,
}
/// One workflow step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Step name.
    pub name: String,
    /// Step payload.
    #[serde(flatten)]
    pub kind: StepKind,
}
/// Step payload variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Validate names, pins, job references, and step payloads.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.name.trim().is_empty() {
            return Err(ContractError::identity("workflow.name", "empty_name"));
        }
        if self.permissions.contents != "read" || self.permissions.actions != "read" {
            return Err(ContractError::identity(
                "workflow.permissions",
                "must_be_read_read",
            ));
        }
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
        for (id, job) in &self.jobs {
            job.validate(id, &ids)?;
        }
        Ok(())
    }
}
impl Job {
    /// Validate one job against the known job-ID set.
    fn validate(&self, id: &str, ids: &BTreeSet<&str>) -> Result<(), ContractError> {
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
/// Check for a literal versioned label (no `latest` aliases or expressions).
fn is_pinned_label(label: &str) -> bool {
    !label.is_empty()
        && !label.contains("${{")
        && !label.contains("latest")
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
}
