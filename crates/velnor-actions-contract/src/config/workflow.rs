use super::rust_suites::WorkflowDispatchConfig;
use super::rust_suites_validate::validate_preserve_workflows;
use super::workflow_flags::{default_true, is_false, is_true};
pub use super::workflow_timeouts::{
    default_crate_timeout_minutes, default_dependency_seed_timeout_minutes,
    default_plan_timeout_minutes,
};
use super::workflow_timeouts::{
    is_default_crate_timeout, is_default_dependency_seed_timeout, is_default_plan_timeout,
};
use crate::config::{TofuApplyConfig, WorkflowLintConfig, WorkflowTask};
use crate::errors::ContractError;
use crate::workflow::ValidatorKind;
use serde::{Deserialize, Serialize};

pub const LATEST_RUNNER_LABEL: &str = "ubuntu-26.04";

pub const RUNNER_LABEL_CATALOG: [&str; 6] = [
    "ubuntu-22.04",
    "ubuntu-24.04",
    "ubuntu-26.04",
    "ubuntu-22.04-arm",
    "ubuntu-24.04-arm",
    "ubuntu-26.04-arm",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowConfig {
    pub name: String,

    pub policy: WorkflowPolicy,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,

    pub generator_validation: GeneratorValidation,

    pub max_parallel_jobs: u32,

    #[serde(
        default = "default_plan_timeout_minutes",
        skip_serializing_if = "is_default_plan_timeout"
    )]
    pub plan_timeout_minutes: u16,

    #[serde(
        default = "default_crate_timeout_minutes",
        skip_serializing_if = "is_default_crate_timeout"
    )]
    pub crate_timeout_minutes: u16,

    #[serde(
        default = "default_dependency_seed_timeout_minutes",
        skip_serializing_if = "is_default_dependency_seed_timeout"
    )]
    pub dependency_seed_timeout_minutes: u16,

    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub mise_quiet: bool,

    #[serde(default, skip_serializing_if = "is_false")]
    pub checkout_lfs: bool,

    #[serde(default)]
    pub pull_request_cache_policy: PullRequestCachePolicy,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_label: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<WorkflowTask>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_suites: Vec<super::MiseTaskSuite>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tofu_apply: Option<TofuApplyConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_completion: Option<super::ReviewCompletionConfig>,

    #[serde(default, skip_serializing_if = "VerifyConfig::is_empty")]
    pub verify: VerifyConfig,

    #[serde(default, skip_serializing_if = "WorkflowLintConfig::is_default")]
    pub lint: WorkflowLintConfig,

    #[serde(default, skip_serializing_if = "WorkflowDispatchConfig::is_empty")]
    pub dispatch: WorkflowDispatchConfig,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preserve_workflows: Vec<String>,
}

const RETIRED_ZIZMOR_JOB: &str = "zizmor";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyConfig {
    #[serde(default)]
    pub jobs: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict_json_files: Option<Vec<String>>,

    #[serde(default)]
    pub lychee_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum PullRequestCachePolicy {
    #[default]
    ReadOnly,

    SameRepositoryScoped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkflowPolicy {
    ConsumerV1,

    VelnorRepositoryV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GeneratorValidation {
    Bootstrap,

    Candidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerSelection {
    LatestDefault,

    ConfigOverride,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VelnorSupportWorkflow {
    pub validators: Vec<ValidatorKind>,
}

impl WorkflowPolicy {
    #[must_use]
    pub fn support_workflow(&self) -> VelnorSupportWorkflow {
        let validators = match self {
            Self::ConsumerV1 => Vec::new(),
            Self::VelnorRepositoryV1 => ValidatorKind::repository_validators().to_vec(),
        };
        VelnorSupportWorkflow { validators }
    }
}

impl WorkflowConfig {
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
        crate::workflow::timeout::JobTimeout::check_config(
            u32::from(self.crate_timeout_minutes),
            file,
            "workflow.crate_timeout_minutes",
        )?;
        crate::workflow::timeout::JobTimeout::check_config(
            u32::from(self.plan_timeout_minutes),
            file,
            "workflow.plan_timeout_minutes",
        )?;
        crate::workflow::timeout::JobTimeout::check_config(
            u32::from(self.dependency_seed_timeout_minutes),
            file,
            "workflow.dependency_seed_timeout_minutes",
        )?;
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
        task_suites_validate::validate(self, file)?;
        if let Some(completion) = &self.review_completion {
            completion.validate(file)?;
        }
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
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let mut seen = std::collections::BTreeSet::new();
        for job in &self.jobs {
            if job == RETIRED_ZIZMOR_JOB {
                return Err(ContractError::config(
                    file,
                    "workflow.verify.jobs",
                    "zizmor_retired:jactionlint covers the zizmor audits; set [workflow.lint] profile = \"default\" and remove \"zizmor\" (hard decision 10)".to_owned(),
                ));
            }
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
                if !workflow_validation_helpers::valid_json_path(path) || !paths.insert(path) {
                    return Err(ContractError::config(
                        file,
                        "workflow.verify.strict_json_files",
                        format!("invalid_or_duplicate_path:{path}"),
                    ));
                }
            }
        }
        for arg in &self.lychee_args {
            if !workflow_validation_helpers::valid_lychee_arg(arg) {
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

#[path = "workflow_task_suites_validate.rs"]
mod task_suites_validate;

#[path = "workflow_validation_helpers.rs"]
mod workflow_validation_helpers;
