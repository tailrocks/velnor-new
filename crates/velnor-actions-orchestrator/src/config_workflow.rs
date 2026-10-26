// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_actions_contract::{
    GeneratorValidation, PullRequestCachePolicy, VerifyConfig, WorkflowConfig, WorkflowPolicy,
    WorkflowTask,
};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PartialWorkflow {
    name: Option<String>,
    policy: Option<WorkflowPolicy>,
    default_branch: Option<String>,
    verification_branch: Option<String>,
    generator_validation: Option<GeneratorValidation>,
    max_parallel_jobs: Option<u32>,
    plan_timeout_minutes: Option<u16>,
    crate_timeout_minutes: Option<u16>,
    dependency_seed_timeout_minutes: Option<u16>,
    mise_quiet: Option<bool>,
    checkout_lfs: Option<bool>,
    #[serde(default)]
    pull_request_cache_policy: PullRequestCachePolicy,
    runner_label: Option<String>,
    #[serde(default)]
    tasks: Vec<WorkflowTask>,
    #[serde(default)]
    task_suites: Vec<velnor_actions_contract::MiseTaskSuite>,
    tofu_apply: Option<velnor_actions_contract::TofuApplyConfig>,
    review_completion: Option<velnor_actions_contract::config::ReviewCompletionConfig>,
    #[serde(default)]
    verify: VerifyConfig,
    #[serde(default)]
    lint: velnor_actions_contract::config::WorkflowLintConfig,
    #[serde(default)]
    dispatch: velnor_actions_contract::WorkflowDispatchConfig,
    #[serde(default)]
    preserve_workflows: Vec<String>,
}

impl PartialWorkflow {
    pub(super) fn materialize(self) -> WorkflowConfig {
        WorkflowConfig {
            name: self.name.unwrap_or_else(|| "CI".to_owned()),
            policy: self.policy.unwrap_or(WorkflowPolicy::ConsumerV1),
            default_branch: self.default_branch,
            verification_branch: self.verification_branch,
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
