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
    WorkflowRoutingOverrides, validate_generated_workflow_calls_with_ir,
    validate_generated_workflow_calls_with_ir_and_overrides,
    validate_generated_workflow_calls_with_native_preview_ir,
    validate_generated_workflow_calls_with_native_preview_ir_and_overrides,
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
    if crate::homebrew::source_policy::requires_native_generation_policy(
        prep.config.distribution.homebrew_tap.as_ref(),
    ) {
        crate::homebrew::source_policy::check_preparation(prep)?;
    }
    let version = env!("CARGO_PKG_VERSION");
    let mise = resolve_mise_setup(&prep.config, &prep.runner_label)?;
    let verifiers = crate::workflow::verifier_actions::VerifierActionSet::build(
        &prep.emission_catalog(),
        version,
    )?;
    let WorkflowPreparation {
        routing,
        ir,
        rendered,
        lint_config,
    } = prepare_workflow(prep, dispatch, &mise, version, &verifiers)?;
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
    let (schema2_files, final_routing, routing_overrides) =
        crate::routing::extra_files(&prep.config, &prep.emission_catalog(), version, &routing)?;
    extra.extend(schema2_files);
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
    extra.extend(verifiers.files());
    let mut audited_files = extra.clone();
    audited_files.push(RenderedFile {
        path: velnor_actions_workflow_renderer::render::WORKFLOW_PATH.to_owned(),
        bytes: workflow.clone(),
    });
    super::rust_install_audit::audit_with_verifiers(&audited_files, &verifiers)?;
    let tree = render_tree_with_claude_mode(
        &workflow,
        &lint_config,
        &extra,
        version,
        prep.config.agents.claude_md,
    )?;
    let has_native_preview = prep
        .config
        .stacks
        .rust
        .as_ref()
        .is_some_and(|rust| rust.binary_release.preview.enabled);
    validate_final_routing_with_overrides(
        &tree.files,
        &final_routing,
        &ir,
        &routing_overrides,
        has_native_preview,
    )?;
    Ok(tree)
}

fn prepare_workflow(
    prep: &GenerationPreparation,
    dispatch: Option<ExecutionMode>,
    mise: &velnor_actions_workflow_renderer::render::MiseSetup,
    version: &str,
    verifiers: &crate::workflow::verifier_actions::VerifierActionSet,
) -> Result<WorkflowPreparation, OrchestratorError> {
    let routing = workflow_routing_policy(&prep.config, dispatch)?;
    let qualification_sources = crate::crate_job_ids::qualification_job_sources_for_ir(
        &prep.discovery.proposals,
        &prep.workflow.ir,
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let mut ir = expand_workflow_with_qualification_sources(
        &prep.workflow.ir,
        &prep.config,
        dispatch,
        &qualification_sources,
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    verifiers.lower_workflow_calls(&mut ir, &prep.workflow.context.workflow_tasks)?;
    let mut render_context = prep.workflow.context.clone();
    render_context.scale_set_selector = scale_set_selector_for_dispatch(&prep.config, dispatch)?;
    let routing = routing.with_verified_workflow_tasks(
        &ir,
        &render_context.workflow_tasks,
        &render_context.checkout_uses,
    )?;
    let job_workflow_ids = routing
        .is_both()
        .then(|| {
            crate::crate_job_ids::typed_rust_lane_job_ids_for_jobs(
                &prep.discovery.proposals,
                &ir.jobs,
            )
            .map_err(|err| OrchestratorError::Contract {
                problem: err.to_string(),
            })
        })
        .transpose()?;
    let mut lint_input = prep.workflow.lint_config.clone();
    lint_input.runner_labels = super::runner_labels::collect(
        &prep.config,
        &ir,
        render_context.scale_set_selector.as_ref(),
    );
    let rendered =
        velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared_with_routing_and_job_workflows(
            &ir,
            prep.config.workflow.policy,
            prep.workflow.support.as_ref(),
            &render_context,
            mise,
            &routing,
            job_workflow_ids.as_ref(),
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

fn validate_final_routing_with_overrides(
    files: &[RenderedFile],
    policy: &velnor_actions_workflow_renderer::WorkflowRoutingPolicy,
    workflow: &velnor_actions_contract::WorkflowIr,
    overrides: &WorkflowRoutingOverrides,
    native_preview: bool,
) -> Result<(), OrchestratorError> {
    if overrides.hosted_only_paths().next().is_none() {
        if native_preview {
            validate_generated_workflow_calls_with_native_preview_ir(files, policy, workflow)?;
        } else {
            validate_final_routing(files, policy, workflow)?;
        }
        return Ok(());
    }
    if native_preview {
        validate_generated_workflow_calls_with_native_preview_ir_and_overrides(
            files, policy, workflow, overrides,
        )?;
    } else {
        validate_generated_workflow_calls_with_ir_and_overrides(
            files, policy, workflow, overrides,
        )?;
    }
    Ok(())
}
