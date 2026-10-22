//! Partial workflow parsing and default materialization.

use serde::Deserialize;
use velnor_actions_contract::{
    GeneratorValidation, PullRequestCachePolicy, VerifyConfig, WorkflowConfig, WorkflowPolicy,
    WorkflowTask,
};

/// Workflow section with every value optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PartialWorkflow {
    /// Display name.
    name: Option<String>,
    /// Workflow policy.
    policy: Option<WorkflowPolicy>,
    /// Default branch override.
    default_branch: Option<String>,
    /// Generator validation mode.
    generator_validation: Option<GeneratorValidation>,
    /// Maximum parallel matrix jobs.
    max_parallel_jobs: Option<u32>,
    /// Planner-job timeout in minutes.
    plan_timeout_minutes: Option<u16>,
    /// Crate-job timeout in minutes.
    crate_timeout_minutes: Option<u16>,
    /// Dependency-seed job timeout in minutes.
    dependency_seed_timeout_minutes: Option<u16>,
    /// Workflow-level Mise silencing override.
    mise_quiet: Option<bool>,
    /// Git LFS objects in generated checkouts.
    checkout_lfs: Option<bool>,
    /// Pull-request cache-write policy.
    #[serde(default)]
    pull_request_cache_policy: PullRequestCachePolicy,
    /// Pinned runner-label override.
    runner_label: Option<String>,
    /// One tagged task inventory for all explicit workflow work.
    #[serde(default)]
    tasks: Vec<WorkflowTask>,
    /// Standalone compile-capable Mise task suites.
    #[serde(default)]
    task_suites: Vec<velnor_actions_contract::MiseTaskSuite>,
    /// Optional protected post-merge `OpenTofu` apply declaration.
    tofu_apply: Option<velnor_actions_contract::TofuApplyConfig>,
    /// Optional closed review completion emitter.
    review_completion: Option<velnor_actions_contract::config::ReviewCompletionConfig>,
    /// Consumer-selected verification jobs.
    #[serde(default)]
    verify: VerifyConfig,
    /// `[workflow.lint]`: jactionlint profile.
    #[serde(default)]
    lint: velnor_actions_contract::config::WorkflowLintConfig,
    /// Optional `workflow_dispatch` inputs for suite workflows.
    #[serde(default)]
    dispatch: velnor_actions_contract::WorkflowDispatchConfig,
    /// Repository-owned workflow files preserved across generation.
    #[serde(default)]
    preserve_workflows: Vec<String>,
}

impl PartialWorkflow {
    /// Fill workflow defaults.
    pub(super) fn materialize(self) -> WorkflowConfig {
        WorkflowConfig {
            name: self.name.unwrap_or_else(|| "CI".to_owned()),
            policy: self.policy.unwrap_or(WorkflowPolicy::ConsumerV1),
            default_branch: self.default_branch,
            generator_validation: self
                .generator_validation
                .unwrap_or(GeneratorValidation::Bootstrap),
            max_parallel_jobs: self.max_parallel_jobs.unwrap_or(2),
            plan_timeout_minutes: self
                .plan_timeout_minutes
                .unwrap_or_else(velnor_actions_contract::config::default_plan_timeout_minutes),
            crate_timeout_minutes: self
                .crate_timeout_minutes
                .unwrap_or_else(velnor_actions_contract::config::default_crate_timeout_minutes),
            dependency_seed_timeout_minutes: self.dependency_seed_timeout_minutes.unwrap_or_else(
                velnor_actions_contract::config::default_dependency_seed_timeout_minutes,
            ),
            mise_quiet: self.mise_quiet.unwrap_or(true),
            checkout_lfs: self.checkout_lfs.unwrap_or(false),
            pull_request_cache_policy: self.pull_request_cache_policy,
            runner_label: self.runner_label,
            tasks: self.tasks,
            task_suites: self.task_suites,
            tofu_apply: self.tofu_apply,
            review_completion: self.review_completion,
            verify: self.verify,
            lint: self.lint,
            dispatch: self.dispatch,
            preserve_workflows: self.preserve_workflows,
        }
    }
}
