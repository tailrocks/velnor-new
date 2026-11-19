// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{
    ExecutionMode, ScaleSetSelector, ValidatorKind, VelnorConfig,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::{
    CanonicalRuntimeInputs, RenderContext, ValidatorCommand,
};
use velnor_actions_workflow_renderer::steps::REQUEST_DIR_PREFIX;
use velnor_actions_workflow_renderer::verification_jobs::WorkflowTaskPolicy;

use crate::OrchestratorError;

use super::REQUEST_DIR;

#[path = "tool_cache_pins.rs"]
mod tool_cache_pins;

pub(super) struct RenderContextInputs<'a> {
    pub(super) authority_steps: velnor_actions_workflow_renderer::render::CanonicalAuthoritySteps,
    pub(super) runtime: CanonicalRuntimeInputs,
    pub(super) config: &'a VelnorConfig,
    pub(super) label: &'a str,
    pub(super) generator_version: &'a str,
    pub(super) catalog: &'a ToolCatalog,
    pub(super) workflow_tasks: Vec<WorkflowTaskPolicy>,
    pub(super) verify: &'a [ValidatorKind],
}

pub(super) fn render_context(
    input: RenderContextInputs<'_>,
) -> Result<RenderContext, OrchestratorError> {
    debug_assert!(REQUEST_DIR.starts_with(REQUEST_DIR_PREFIX));
    let validator_commands = validator_commands(&input)?;
    let scale_set_selector = scale_set_selector(input.config, None)?;
    let runtime = input.runtime;
    Ok(RenderContext {
        authority_steps: input.authority_steps,
        tool_pins: tool_cache_pins::catalog_tool_pins(input.catalog),
        generator_version: input.generator_version.to_owned(),
        controller: runtime.controller().clone(),
        runs_on: input.label.to_owned(),
        scale_set_selector,
        request_dir: runtime.request_dir().to_owned(),
        checkout_uses: runtime.checkout_uses().to_owned(),
        validator_commands,
        preseed: runtime.preseed(),
        workflow_tasks: input.workflow_tasks,
        lane_plan: None,
        pull_request_cache_policy: input.config.workflow.pull_request_cache_policy,
        mise_quiet: input.config.workflow.mise_quiet,
        plan_consumer_env: runtime.plan_consumer_env().clone(),
        max_parallel_jobs: input.config.workflow.max_parallel_jobs,
        non_inputs: input
            .config
            .stacks
            .rust
            .as_ref()
            .map(|rust| rust.non_inputs.clone())
            .unwrap_or_default(),
    })
}

fn validator_commands(
    input: &RenderContextInputs<'_>,
) -> Result<Vec<ValidatorCommand>, OrchestratorError> {
    let mut validator_commands = Vec::new();
    crate::verify::push_verify_commands(
        &mut validator_commands,
        input.verify,
        &input.config.workflow.verify,
    )?;
    Ok(validator_commands)
}

pub(super) fn scale_set_selector(
    config: &VelnorConfig,
    dispatch: Option<ExecutionMode>,
) -> Result<Option<ScaleSetSelector>, OrchestratorError> {
    let selector = config
        .execution
        .as_ref()
        .map(|execution| {
            execution
                .validate("config.toml")
                .map_err(|error| OrchestratorError::Contract {
                    problem: error.to_string(),
                })?;
            let mut effective = execution.clone();
            if let Some(mode) = dispatch {
                effective.mode = Some(mode);
            }
            if !effective.emits_scale_set_selector() {
                return Ok(None);
            }
            effective
                .scale_selector()
                .map(Some)
                .map_err(|error| OrchestratorError::Contract {
                    problem: error.to_string(),
                })
        })
        .transpose()?;
    Ok(selector.flatten())
}
