//! In-memory workflow and generated-file rendering.

#[path = "review_completion_workflow_emit.rs"]
mod review_completion_workflow_emit;

use velnor_actions_actionlint::render_jactionlint_yaml;
use velnor_actions_contract::{
    ExecutionMode, WorkflowIr, expand_workflow_with_qualification_sources,
};
use velnor_actions_workflow_renderer::render::{RenderedFile, RenderedTree, RenderedWorkflow};
use velnor_actions_workflow_renderer::steps::rehead_lint_config_marker;
use velnor_actions_workflow_renderer::tree::render_tree_with_claude_mode;
use velnor_actions_workflow_renderer::{
    validate_generated_workflow_calls_with_ir,
    validate_generated_workflow_calls_with_native_preview_ir,
};

use crate::OrchestratorError;
use crate::pins::resolve_mise_setup;
use crate::prepare::GenerationPreparation;
use crate::routing::workflow_routing_policy;
use crate::workflow::scale_set_selector_for_dispatch;

struct WorkflowPreparation {
    routing: velnor_actions_workflow_renderer::WorkflowRoutingPolicy,
    ir: WorkflowIr,
    rendered: RenderedWorkflow,
    lint_config: String,
}

#[cfg(test)]
#[path = "generate_render_tests.rs"]
mod tests;

/// Render base files plus release extras into the marker-checked tree, in memory only.
pub(super) fn render_all(
    prep: &GenerationPreparation,
    dispatch: Option<ExecutionMode>,
) -> Result<RenderedTree, OrchestratorError> {
    let version = env!("CARGO_PKG_VERSION");
    let mise = resolve_mise_setup(&prep.config, &prep.runner_label)?;
    let WorkflowPreparation {
        routing,
        ir,
        rendered,
        lint_config,
    } = prepare_workflow(prep, dispatch, &mise, version)?;
    let workflow = rendered.yaml;
    let mut extra = crate::release_emit::release_files(prep, &mise, &routing)?;
    extra.extend(crate::publish_emit::publish_files(prep, &mise, &routing)?);
    extra.extend(crate::binary_release_emit::files(prep, &routing)?);
    extra.extend(crate::binary_preview_emit::files(prep, &routing)?);
    extra.extend(crate::freshness_emit::freshness_files(prep, &routing)?);
    extra.extend(crate::freshness_emit::upgrades::upgrades_files(
        prep, &routing,
    )?);
    extra.extend(crate::owned_tool_publication::files_with_routing(
        prep, &routing,
    )?);
    extra.extend(crate::tofu_apply_emit::tofu_apply_files(prep, &routing)?);
    extra.extend(review_completion_workflow_emit::files(prep, &routing)?);
    extra.extend(crate::routing::extra_files(
        &prep.config,
        &prep.emission_catalog(),
        version,
        &routing,
    )?);
    extra.extend(crate::rust_suites_emit::suite_workflow_files(
        prep, &routing,
    )?);
    extra.extend(super::mise_task_suites_emit::suite_workflow_files(prep)?);
    if let Some(dco) = &prep.config.dco {
        extra.push(velnor_actions_workflow_renderer::dco::render_dco_yml(
            dco, version,
        )?);
    }
    extra.extend(rendered.shared);
    super::rust_install_audit::audit(&extra)?;
    super::rust_install_audit::audit(&[RenderedFile {
        path: velnor_actions_workflow_renderer::render::WORKFLOW_PATH.to_owned(),
        bytes: workflow.clone(),
    }])?;
    let tree = render_tree_with_claude_mode(
        &workflow,
        &lint_config,
        &extra,
        version,
        prep.config.agents.claude_md,
    )?;
    if prep
        .config
        .stacks
        .rust
        .as_ref()
        .is_some_and(|rust| rust.binary_release.preview.enabled)
    {
        validate_generated_workflow_calls_with_native_preview_ir(&tree.files, &routing, &ir)?;
    } else {
        validate_final_routing(&tree.files, &routing, &ir)?;
    }
    Ok(tree)
}

fn prepare_workflow(
    prep: &GenerationPreparation,
    dispatch: Option<ExecutionMode>,
    mise: &velnor_actions_workflow_renderer::render::MiseSetup,
    version: &str,
) -> Result<WorkflowPreparation, OrchestratorError> {
    let routing = workflow_routing_policy(&prep.config, dispatch)?;
    let qualification_sources = crate::crate_job_ids::qualification_job_sources_for_ir(
        &prep.discovery.proposals,
        &prep.workflow.ir,
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let ir = expand_workflow_with_qualification_sources(
        &prep.workflow.ir,
        &prep.config,
        dispatch,
        &qualification_sources,
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let mut render_context = prep.workflow.context.clone();
    render_context.scale_set_selector = scale_set_selector_for_dispatch(&prep.config, dispatch)?;
    let mut lint_input = prep.workflow.lint_config.clone();
    lint_input.runner_labels = super::runner_labels::collect(
        &prep.config,
        &ir,
        render_context.scale_set_selector.as_ref(),
    );
    let rendered =
        velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared_with_routing(
            &ir,
            prep.config.workflow.policy,
            prep.workflow.support.as_ref(),
            &render_context,
            mise,
            &routing,
        )?;
    let lint_config = rehead_lint_config_marker(&render_jactionlint_yaml(&lint_input)?, version)?;
    Ok(WorkflowPreparation {
        routing,
        ir,
        rendered,
        lint_config,
    })
}

fn validate_final_routing(
    files: &[RenderedFile],
    policy: &velnor_actions_workflow_renderer::WorkflowRoutingPolicy,
    workflow: &velnor_actions_contract::WorkflowIr,
) -> Result<(), OrchestratorError> {
    validate_generated_workflow_calls_with_ir(files, policy, workflow)?;
    Ok(())
}
