//! Workflow section of `.velnor/config.toml`: naming, policy, validators.
use serde::{Deserialize, Serialize};

use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract_release::targets::RUNNER_LABEL_CATALOG;

use crate::config::{ArtifactBuildTask, VerificationTask};

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
    /// Sorted build tasks with exact, bounded outputs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_tasks: Vec<ArtifactBuildTask>,
    /// Explicit `ConsumerV1` verification jobs.
    #[serde(default, skip_serializing_if = "VerifyConfig::is_empty")]
    pub verify: VerifyConfig,
}

/// Explicit, closed `ConsumerV1` verification selection.
///
/// The selector is optional for existing configurations. This source slice
/// implements only zizmor; other PR98 IDs fail validation until their
/// production job and Required wiring are implemented.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyConfig {
    /// Stable PR98 verification job IDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jobs: Vec<String>,
}

impl VerifyConfig {
    fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    fn validate(&self, file: &str, policy: WorkflowPolicy) -> Result<(), ContractError> {
        if self.jobs.is_empty() {
            return Ok(());
        }
        if policy != WorkflowPolicy::ConsumerV1 {
            return Err(ContractError::config(
                file,
                "workflow.verify",
                "consumer_policy_required",
            ));
        }

        let mut seen = std::collections::BTreeSet::new();
        for job in &self.jobs {
            if !seen.insert(job.as_str()) {
                return Err(ContractError::config(
                    file,
                    "workflow.verify.jobs",
                    format!("duplicate_verify_job:{job}"),
                ));
            }
            if job != "zizmor" {
                return Err(ContractError::config(
                    file,
                    "workflow.verify.jobs",
                    format!("unsupported_verify_job:{job}"),
                ));
            }
        }
        Ok(())
    }
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

/// Independent validator jobs (P05-6: no Policy umbrella).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorKind {
    /// Repository-structure lint.
    Alint,
    /// Dependency/security audit.
    CargoDeny,
    /// Unused-dependency scan.
    CargoMachete,
    /// Workflow-file lint.
    Actionlint,
    /// Workflow security audit.
    Zizmor,
}

impl ValidatorKind {
    /// Stable unbranded job ID.
    #[must_use]
    pub fn job_id(&self) -> &'static str {
        match self {
            Self::Alint => "alint",
            Self::CargoDeny => "cargo-deny",
            Self::CargoMachete => "cargo-machete",
            Self::Actionlint => "actionlint",
            Self::Zizmor => "zizmor",
        }
    }

    /// Stable human-readable display name.
    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Alint => "Alint",
            Self::CargoDeny => "Cargo Deny",
            Self::CargoMachete => "Cargo Machete",
            Self::Actionlint => "Actionlint",
            Self::Zizmor => "Zizmor",
        }
    }

    /// Every validator kind in emission order.
    #[must_use]
    pub fn all() -> [Self; 5] {
        [
            Self::Alint,
            Self::CargoDeny,
            Self::CargoMachete,
            Self::Actionlint,
            Self::Zizmor,
        ]
    }

    /// Velnor-repository validators emitted as support jobs.
    ///
    /// Actionlint is always-on base IR on both policies, never support.
    #[must_use]
    pub fn repository_validators() -> [Self; 4] {
        [
            Self::Alint,
            Self::CargoDeny,
            Self::CargoMachete,
            Self::Zizmor,
        ]
    }
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
        self.verify.validate(file, self.policy)?;
        self.validate_tasks(file)?;
        self.validate_artifact_tasks(file)?;
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

    /// Validate artifact tasks and prevent overlap with verification jobs.
    /// # Errors
    fn validate_artifact_tasks(&self, file: &str) -> Result<(), ContractError> {
        let verification_ids: std::collections::BTreeSet<&str> =
            self.tasks.iter().map(|task| task.id.as_str()).collect();
        let mut previous = None;
        for task in &self.artifact_tasks {
            task.validate(file)?;
            if previous.is_some_and(|id: &str| id >= task.id.as_str()) {
                let problem = if previous == Some(task.id.as_str()) {
                    format!("duplicate_artifact_task:{}", task.id)
                } else {
                    "artifact_tasks_must_be_sorted_by_id".to_owned()
                };
                return Err(ContractError::config(
                    file,
                    "workflow.artifact_tasks",
                    problem,
                ));
            }
            if verification_ids.contains(task.id.as_str()) {
                return Err(ContractError::config(
                    file,
                    "workflow.artifact_tasks",
                    format!("task_id_overlaps_verification_task:{}", task.id),
                ));
            }
            previous = Some(task.id.as_str());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
