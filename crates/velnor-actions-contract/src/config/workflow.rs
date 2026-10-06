//! Workflow section of `.velnor/config.toml`: naming, policy, runner labels.
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
    /// Pinned older runner-label override; omit for latest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_label: Option<String>,
    /// Sorted, unique typed task graph shared by verification and build variants.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<WorkflowTask>,
    /// Optional protected post-merge `OpenTofu` apply workflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tofu_apply: Option<TofuApplyConfig>,
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

#[cfg(test)]
mod tests {
    use super::{GeneratorValidation, WorkflowConfig, WorkflowPolicy};
    use crate::config::{
        BuildTask, BuildTaskRunner, NativeImageCachePolicy, NativeImagePlatform, NativeImageTask,
        VerificationRunner, VerificationTask, WorkflowTask,
    };

    /// Workflow config carrying `name`, all else default.
    fn named(name: &str) -> WorkflowConfig {
        WorkflowConfig {
            name: name.to_owned(),
            policy: WorkflowPolicy::ConsumerV1,
            default_branch: None,
            generator_validation: GeneratorValidation::Bootstrap,
            max_parallel_jobs: 2,
            runner_label: None,
            tasks: Vec::new(),
            tofu_apply: None,
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
    fn generic_workflow_runner_catalog_is_linux_only() {
        assert!(
            super::RUNNER_LABEL_CATALOG
                .iter()
                .all(|label| label.starts_with("ubuntu-"))
        );
        let mut config = named("CI");
        config.runner_label = Some("macos-15".to_owned());
        assert!(
            config
                .validate("config.toml")
                .expect_err("macOS checks use their separate typed runner config")
                .to_string()
                .contains("unsupported_label:macos-15")
        );
    }

    #[test]
    fn workflow_tasks_require_sorted_unique_safe_ids() {
        let make = |id: &str| {
            WorkflowTask::Verification(VerificationTask {
                id: id.to_owned(),
                mise_task: format!("check-{id}"),
                runner: VerificationRunner::LinuxX64,
                timeout_minutes: 10,
            })
        };
        let mut valid = named("CI");
        valid.tasks = vec![make("native-format"), make("native-lint")];
        assert!(valid.validate("config.toml").is_ok());

        valid.tasks.reverse();
        let error = valid.validate("config.toml").expect_err("unsorted fails");
        assert!(error.to_string().contains("tasks_must_be_sorted_by_id"));

        valid.tasks = vec![make("native-lint"), make("native-lint")];
        let error = valid.validate("config.toml").expect_err("duplicate fails");
        assert!(error.to_string().contains("duplicate_workflow_task_id"));

        valid.tasks = vec![make("required")];
        let error = valid
            .validate("config.toml")
            .expect_err("reserved ID fails");
        assert!(error.to_string().contains("bad_workflow_task_id"));
    }

    #[test]
    fn workflow_task_variants_share_sorted_unique_ids_and_build_bound() {
        let make_build = |id: &str| {
            WorkflowTask::Build(BuildTask {
                id: id.to_owned(),
                mise_task: "desktop-ci".to_owned(),
                tools: vec!["mr-boxington".to_owned(), "rust".to_owned()],
                runner: BuildTaskRunner::Macos26Arm64,
                timeout_minutes: 120,
                cargo_build_jobs: 2,
                nextest_test_threads: 2,
            })
        };
        let mut valid = named("CI");
        valid.tasks = vec![make_build("native-desktop")];
        assert!(valid.validate("config.toml").is_ok());

        valid.tasks = vec![make_build("native-desktop"), make_build("native-ios")];
        let error = valid
            .validate("config.toml")
            .expect_err("multiple native build tasks fail");
        assert!(
            error
                .to_string()
                .contains("more_than_one_native_build_task")
        );

        valid.tasks = vec![make_build("required")];
        let error = valid
            .validate("config.toml")
            .expect_err("reserved build task ID fails");
        assert!(error.to_string().contains("bad_workflow_task_id"));

        valid.tasks = vec![
            make_build("native-desktop"),
            WorkflowTask::Verification(VerificationTask {
                id: "native-desktop".to_owned(),
                mise_task: "desktop-format-check".to_owned(),
                runner: VerificationRunner::MacosArm64,
                timeout_minutes: 10,
            }),
        ];
        let error = valid
            .validate("config.toml")
            .expect_err("duplicate IDs across variants fail");
        assert!(error.to_string().contains("duplicate_workflow_task_id"));
    }

    #[test]
    fn default_branch_rejects_yaml_and_git_injection() {
        for branch in [
            "",
            "feature/x y",
            "main\non: [push]",
            "main;git status",
            "${{ github.ref }}",
            "release/../main",
            "main.lock",
        ] {
            let mut config = named("CI");
            config.default_branch = Some(branch.to_owned());
            let err = config
                .validate("config.toml")
                .expect_err("malformed default branch must fail");
            assert!(err.to_string().contains("malformed_branch"), "{err}");
        }

        let mut config = named("CI");
        config.default_branch = Some("release/1.2".to_owned());
        assert_eq!(config.validate("config.toml"), Ok(()));
    }

    #[test]
    fn native_image_tasks_share_ids_and_have_one_hosted_capability_slot() {
        let image = |id: &str| {
            WorkflowTask::NativeImage(NativeImageTask {
                id: id.to_owned(),
                platform: NativeImagePlatform::LinuxArm64,
                script: "maintained-image-build/arm64-image-validation.sh".to_owned(),
                timeout_minutes: 60,
                cache: NativeImageCachePolicy::TaskOwnedBuilder,
            })
        };
        let mut valid = named("CI");
        valid.tasks = vec![image("architect-arm64-image")];
        assert!(valid.validate("config.toml").is_ok());

        valid.tasks = vec![image("architect-arm64-image"), image("other-image")];
        let error = valid
            .validate("config.toml")
            .expect_err("multiple image tasks exceed the one-host resource envelope");
        assert!(
            error
                .to_string()
                .contains("more_than_one_native_image_task")
        );

        valid.tasks = vec![image("required")];
        let error = valid
            .validate("config.toml")
            .expect_err("image tasks use the workflow-wide ID namespace");
        assert!(error.to_string().contains("bad_workflow_task_id"));
    }
}
