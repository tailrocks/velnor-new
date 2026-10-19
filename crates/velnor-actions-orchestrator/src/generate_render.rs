//! In-memory workflow and generated-file rendering.

use velnor_actions_actionlint::render_actionlint_yaml;
use velnor_actions_contract::{ExecutionMode, expand_workflow_with_qualification_sources};
use velnor_actions_workflow_renderer::render::{RenderedFile, RenderedTree};
use velnor_actions_workflow_renderer::steps::rehead_actionlint_marker;
use velnor_actions_workflow_renderer::tree::render_tree_with_claude_mode;
use velnor_actions_workflow_renderer::validate_generated_workflow_calls_with_ir;

use crate::OrchestratorError;
use crate::pins::resolve_mise_setup;
use crate::prepare::GenerationPreparation;
use crate::routing::workflow_routing_policy;
use crate::workflow::scale_set_selector_for_dispatch;

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
    let mut actionlint_input = prep.workflow.actionlint.clone();
    actionlint_input.extra_runner_labels = super::runner_labels::collect(
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
            &mise,
            &routing,
        )?;
    let workflow = rendered.yaml;
    let actionlint = render_actionlint_yaml(&actionlint_input)?;
    let actionlint = rehead_actionlint_marker(&actionlint.yaml, version)?;
    let mut extra = crate::release_emit::release_files(prep, &mise, &routing)?;
    extra.extend(crate::binary_release_emit::files(prep, &routing)?);
    extra.extend(crate::binary_preview_emit::files(prep, &routing)?);
    extra.extend(crate::freshness_emit::freshness_files(prep, &routing)?);
    extra.extend(crate::owned_tool_publication::files_with_routing(
        prep, &routing,
    )?);
    extra.extend(crate::tofu_apply_emit::tofu_apply_files(prep, &routing)?);
    extra.extend(crate::review_completion_workflow_emit::files(
        prep, &routing,
    )?);
    extra.extend(crate::routing::extra_files(
        &prep.config,
        &prep.emission_catalog(),
        version,
        &routing,
    )?);
    extra.extend(crate::rust_suites_emit::suite_workflow_files(
        prep, &routing,
    )?);
    if let Some(dco) = &prep.config.dco {
        extra.push(velnor_actions_workflow_renderer::dco::render_dco_yml(
            dco, version,
        )?);
    }
    extra.extend(rendered.shared);
    let tree = render_tree_with_claude_mode(
        &workflow,
        &actionlint,
        &extra,
        version,
        prep.config.agents.claude_md,
    )?;
    validate_final_routing(&tree.files, &routing, &ir)?;
    Ok(tree)
}

fn validate_final_routing(
    files: &[RenderedFile],
    policy: &velnor_actions_workflow_renderer::WorkflowRoutingPolicy,
    workflow: &velnor_actions_contract::WorkflowIr,
) -> Result<(), OrchestratorError> {
    validate_generated_workflow_calls_with_ir(files, policy, workflow)?;
    Ok(())
}
