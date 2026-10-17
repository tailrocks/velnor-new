//! Workflow section of `.velnor/config.toml`: naming, policy, runner labels.
use super::rust_suites::WorkflowDispatchConfig;
use super::rust_suites_validate::validate_preserve_workflows;
use crate::config::{TofuApplyConfig, WorkflowTask};
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
    /// Keep `MISE_QUIET=1` in the generated workflow environment (default true).
    ///
    /// Silence is the default for log hygiene. `false` omits `MISE_QUIET` so
    /// Mise stderr (including the wrapper-dispatch trace) stays visible for
    /// consumers that assert on it. Verification-task documents never carry
    /// `MISE_QUIET` regardless of this flag.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub mise_quiet: bool,
    /// Cache writes from pull requests; same-repository scope requires explicit opt-in.
    #[serde(default)]
    pub pull_request_cache_policy: PullRequestCachePolicy,
    /// Pinned older runner-label override; omit for latest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_label: Option<String>,
    /// Sorted, unique typed task graph shared by verification and build variants.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<WorkflowTask>,
    /// Optional protected post-merge `OpenTofu` apply workflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tofu_apply: Option<TofuApplyConfig>,
    /// Consumer-selected verification jobs; empty disables them all.
    #[serde(default, skip_serializing_if = "VerifyConfig::is_empty")]
    pub verify: VerifyConfig,
    /// Optional `workflow_dispatch` inputs for suite workflows.
    #[serde(default, skip_serializing_if = "WorkflowDispatchConfig::is_empty")]
    pub dispatch: WorkflowDispatchConfig,
    /// Repository-owned workflow files under `.github/workflows/` preserved across generation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preserve_workflows: Vec<String>,
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
    /// Optional explicit JSON file scope for `strict-json`; absent checks all JSON files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict_json_files: Option<Vec<String>>,
    /// Extra Lychee argv entries for the `link-check` validator.
    #[serde(default)]
    pub lychee_args: Vec<String>,
}

/// Cache-write policy for pull-request workflows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum PullRequestCachePolicy {
    /// Pull requests restore caches and never save them.
    #[default]
    ReadOnly,
    /// Same-repository pull requests may save only to pull-request-scoped caches.
    SameRepositoryScoped,
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
            && !crate::is_valid_branch_name(branch)
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
        if let Some(tofu_apply) = &self.tofu_apply {
            tofu_apply.validate(file)?;
            if self.default_branch.is_none() {
                return Err(ContractError::config(
                    file,
                    "workflow.default_branch",
                    "required_for_tofu_apply",
                ));
            }
        }
        self.verify.validate(file)?;
        self.dispatch.validate(file)?;
        validate_preserve_workflows(&self.preserve_workflows, file)?;
        Ok(())
    }

    /// Validate the deterministic workflow-level task inventory.
    /// # Errors
    fn validate_tasks(&self, file: &str) -> Result<(), ContractError> {
        let mut previous = None;
        let mut build_tasks = 0;
        let mut native_image_tasks = 0;
        for task in &self.tasks {
            task.validate(file)?;
            if matches!(task, WorkflowTask::Build(_)) {
                build_tasks += 1;
                if build_tasks > 1 {
                    return Err(ContractError::config(
                        file,
                        "workflow.tasks",
                        "more_than_one_native_build_task",
                    ));
                }
            }
            if matches!(task, WorkflowTask::NativeImage(_)) {
                native_image_tasks += 1;
                if native_image_tasks > 1 {
                    return Err(ContractError::config(
                        file,
                        "workflow.tasks",
                        "more_than_one_native_image_task",
                    ));
                }
            }
            let id = task.id();
            if previous.is_some_and(|previous_id: &str| previous_id >= id) {
                let problem = if previous == Some(id) {
                    format!("duplicate_workflow_task_id:{id}")
                } else {
                    "tasks_must_be_sorted_by_id".to_owned()
                };
                return Err(ContractError::config(file, "workflow.tasks", problem));
            }
            previous = Some(id);
        }
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
        if let Some(files) = &self.strict_json_files {
            if !self.jobs.iter().any(|job| job == "strict-json") || files.is_empty() {
                return Err(ContractError::config(
                    file,
                    "workflow.verify.strict_json_files",
                    "requires_strict_json_and_nonempty_files",
                ));
            }
            let mut paths = std::collections::BTreeSet::new();
            for path in files {
                if !valid_json_path(path) || !paths.insert(path) {
                    return Err(ContractError::config(
                        file,
                        "workflow.verify.strict_json_files",
                        format!("invalid_or_duplicate_path:{path}"),
                    ));
                }
            }
        }
        for arg in &self.lychee_args {
            if !valid_lychee_arg(arg) {
                return Err(ContractError::config(
                    file,
                    "workflow.verify.lychee_args",
                    format!("invalid_or_duplicate_lychee_arg:{arg}"),
                ));
            }
        }
        if !self.lychee_args.is_empty() && !self.jobs.iter().any(|job| job == "link-check") {
            return Err(ContractError::config(
                file,
                "workflow.verify.lychee_args",
                "requires_link_check",
            ));
        }
        Ok(())
    }
}

fn valid_json_path(value: &str) -> bool {
    !value.is_empty()
        && std::path::Path::new(value)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        && !value.starts_with('/')
        && value.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
}

fn valid_lychee_arg(value: &str) -> bool {
    !value.is_empty()
        && value != "--"
        && !value.contains("${{")
        && !value.contains(['$', '`'])
        && !value.chars().any(char::is_control)
        && !value.split_whitespace().any(|word| word == "mise")
}

/// Default `mise_quiet` value: silence stays on.
fn default_true() -> bool {
    true
}

/// Serialization predicate keeping the `mise_quiet` default out of emitted config.
///
/// Serde's `skip_serializing_if` requires `fn(&bool) -> bool`, so the
/// small-`Copy` by-value clippy lint does not apply here.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires fn(&bool) -> bool"
)]
fn is_true(value: &bool) -> bool {
    *value
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;
