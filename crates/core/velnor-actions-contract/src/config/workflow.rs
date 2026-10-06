//! Workflow section of `.velnor/config.toml`: naming, policy, runner labels.
use crate::config::VerificationTask;
use crate::errors::ContractError;
use crate::workflow::ValidatorKind;
use serde::{Deserialize, Serialize};

/// Latest pinned runner label: the default when `workflow.runner_label` is absent.
///
/// Qualified 2026-09-28 (`ubuntu-latest` then pointed at 24.04; 26.04 is the
/// latest versioned LTS with x64 and `-arm` variants).
pub const LATEST_RUNNER_LABEL: &str = "ubuntu-26.04";
/// Exact-match runner-label catalog: versioned LTS labels plus `-arm` variants.
///
/// Aliases (`ubuntu-latest`, `*-latest`) and unlisted labels are rejected.
pub const RUNNER_LABEL_CATALOG: [&str; 6] = [
    "ubuntu-22.04",
    "ubuntu-24.04",
    "ubuntu-26.04",
    "ubuntu-22.04-arm",
    "ubuntu-24.04-arm",
    "ubuntu-26.04-arm",
];

/// Workflow section of `.velnor/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowConfig {
    /// Display name for the generated workflow.
    pub name: String,
    /// Workflow policy.
    pub policy: WorkflowPolicy,
    /// Default branch; required when `origin/HEAD` cannot be resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    /// Generator validation mode.
    pub generator_validation: GeneratorValidation,
    /// Maximum parallel matrix jobs.
    pub max_parallel_jobs: u32,
    /// Pinned older runner-label override; omit for latest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_label: Option<String>,
    /// Sorted, explicit isolated validation jobs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<VerificationTask>,
}

/// Workflow policy selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkflowPolicy {
    /// Default consumer policy; assumes no Velnor-specific files.
    ConsumerV1,
    /// Velnor-repository policy; canonical `tailrocks/velnor-new` only.
    VelnorRepositoryV1,
}

/// Generator validation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GeneratorValidation {
    /// Validate with the locked bootstrap binary.
    Bootstrap,
    /// Validate with the built candidate binary (Velnor only).
    Candidate,
}

/// Runner-label selection provenance recorded in the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerSelection {
    /// Latest pinned label; no override present.
    LatestDefault,
    /// Explicit `workflow.runner_label` override.
    ConfigOverride,
}

/// Support-job set derived from policy plus validation mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VelnorSupportWorkflow {
    /// Repository validators to emit as separate jobs (P05-6: no umbrella).
    pub validators: Vec<ValidatorKind>,
    /// Whether to emit candidate validation.
    pub candidate_validation: bool,
}

impl WorkflowPolicy {
    /// Derive the support-job set for this policy and validation mode.
    #[must_use]
    pub fn support_workflow(&self, validation: GeneratorValidation) -> VelnorSupportWorkflow {
        let validators = match self {
            Self::ConsumerV1 => Vec::new(),
            Self::VelnorRepositoryV1 => ValidatorKind::repository_validators().to_vec(),
        };
        VelnorSupportWorkflow {
            validators,
            candidate_validation: validation == GeneratorValidation::Candidate,
        }
    }
}

impl WorkflowConfig {
    /// Validate the workflow section.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.name.trim().is_empty() {
            return Err(ContractError::config(file, "workflow.name", "empty_name"));
        }
        if self.name.contains("${{") || self.name.chars().any(char::is_control) {
            return Err(ContractError::config(file, "workflow.name", "bad_name"));
        }
        if self.max_parallel_jobs < 1 {
            return Err(ContractError::config(
                file,
                "workflow.max_parallel_jobs",
                "must_be_at_least_one",
            ));
        }
        if let Some(branch) = &self.default_branch
            && (branch.trim().is_empty() || branch.contains(' ') || branch.contains(".."))
        {
            return Err(ContractError::config(
                file,
                "workflow.default_branch",
                "malformed_branch",
            ));
        }
        if let Some(label) = &self.runner_label
            && !RUNNER_LABEL_CATALOG.contains(&label.as_str())
        {
            return Err(ContractError::config(
                file,
                "workflow.runner_label",
                format!("unsupported_label:{label}"),
            ));
        }
        self.validate_tasks(file)?;
        Ok(())
    }

    /// Validate the deterministic workflow-level task inventory.
    /// # Errors
    fn validate_tasks(&self, file: &str) -> Result<(), ContractError> {
        let mut previous = None;
        for task in &self.tasks {
            task.validate(file)?;
            if previous.is_some_and(|id: &str| id >= task.id.as_str()) {
                let problem = if previous == Some(task.id.as_str()) {
                    format!("duplicate_verification_task:{}", task.id)
                } else {
                    "tasks_must_be_sorted_by_id".to_owned()
                };
                return Err(ContractError::config(file, "workflow.tasks", problem));
            }
            previous = Some(task.id.as_str());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
