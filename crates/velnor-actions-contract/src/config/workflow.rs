//! Workflow section of `.velnor/config.toml`: naming, policy, runner labels.
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
    /// Consumer-selected verification jobs; empty disables them all.
    #[serde(default, skip_serializing_if = "VerifyConfig::is_empty")]
    pub verify: VerifyConfig,
}

/// `[workflow.verify]`: config-selected verification jobs.
///
/// Each entry names a [`ValidatorKind::consumer_verify`] job ID; the
/// generator emits one support job per entry on any policy and the
/// required gate covers them. Unknown or duplicated names fail
/// validation closed. Enabled jobs may require repository-owned
/// policy files (`.alint.yml`, `.zizmor.yml`); a missing file fails
/// that job, never silently.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyConfig {
    /// Verification job IDs to emit, in canonical emission order.
    #[serde(default)]
    pub jobs: Vec<String>,
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
        self.verify.validate(file)?;
        Ok(())
    }
}

impl VerifyConfig {
    /// True when no verification job is selected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    /// Validate the verify allowlist: known IDs, no duplicates.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let mut seen = std::collections::BTreeSet::new();
        for job in &self.jobs {
            if ValidatorKind::from_verify_name(job).is_none() {
                return Err(ContractError::config(
                    file,
                    "workflow.verify.jobs",
                    format!("unknown_verify_job:{job}"),
                ));
            }
            if !seen.insert(job) {
                return Err(ContractError::config(
                    file,
                    "workflow.verify.jobs",
                    format!("duplicate_verify_job:{job}"),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{GeneratorValidation, WorkflowConfig, WorkflowPolicy};

    /// Workflow config carrying `name`, all else default.
    fn named(name: &str) -> WorkflowConfig {
        WorkflowConfig {
            name: name.to_owned(),
            policy: WorkflowPolicy::ConsumerV1,
            default_branch: None,
            generator_validation: GeneratorValidation::Bootstrap,
            max_parallel_jobs: 2,
            runner_label: None,
            verify: super::VerifyConfig::default(),
        }
    }

    #[test]
    fn workflow_name_rejects_expressions_and_controls() {
        assert!(named("CI").validate("config.toml").is_ok());
        for name in ["${{ github.ref }}", "a\nb", "a\rb", "a\tb"] {
            let err = named(name)
                .validate("config.toml")
                .expect_err("bad name fails");
            assert!(err.to_string().contains("bad_name"), "{err}");
        }
    }

    #[test]
    fn verify_jobs_accept_known_ids_in_any_order() {
        let mut config = named("CI");
        config.verify.jobs = vec![
            "native-validators".to_owned(),
            "alint".to_owned(),
            "strict-json".to_owned(),
        ];
        assert!(config.validate("config.toml").is_ok());
    }

    #[test]
    fn verify_jobs_reject_unknown_and_duplicates() {
        let mut config = named("CI");
        config.verify.jobs = vec!["bogus".to_owned()];
        let err = config
            .validate("config.toml")
            .expect_err("unknown job fails");
        assert!(
            err.to_string().contains("unknown_verify_job:bogus"),
            "{err}"
        );
        for forbidden in ["cargo-deny", "cargo-machete", "actionlint", "plan"] {
            config.verify.jobs = vec![forbidden.to_owned()];
            let err = config
                .validate("config.toml")
                .expect_err("non-verify ID fails");
            assert!(
                err.to_string().contains("unknown_verify_job"),
                "{forbidden}: {err}"
            );
        }
        config.verify.jobs = vec!["alint".to_owned(), "alint".to_owned()];
        let err = config
            .validate("config.toml")
            .expect_err("duplicate job fails");
        assert!(
            err.to_string().contains("duplicate_verify_job:alint"),
            "{err}"
        );
    }
}
