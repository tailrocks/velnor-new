//! Strict workflow rendering: merged emission plus fail-closed gates.
//!
//! [`render_merged`] emits matrix strategy plus the quoted, marked workflow
//! text from finalized jobs. The strict entrypoints finalize through the
//! jobs crate (Mise setup, staged helpers, plan anchor) before emission.

use std::collections::BTreeMap;

use velnor_actions_contract_config::{ExecutionMode, VelnorSupportWorkflow, WorkflowPolicy};
use velnor_actions_contract_workflow::{CI_WORKFLOW_PATH, Job, WorkflowIr};
use velnor_actions_workflow_document::{artifact_matrix, document, matrix};
use velnor_actions_workflow_jobs::{RenderContext, finalize::finalize_jobs};
use velnor_actions_workflow_steps::{RenderError, setup::MiseSetup, steps};
use velnor_actions_workflow_tree::{marker, yaml::render_yaml};

pub use velnor_actions_workflow_document::lane_share::RenderedWorkflow;

/// Strict render: setup insertion plus staged-helper and anchor gates.
///
/// Every `mise`-invoking job (plus plan/task unconditionally) gains a
/// preceding pinned setup step; internal steps without a preceding
/// Acquire step and anchorless plan jobs fail closed. In pre-seed mode
/// the fixed pre-seed stage step stages instead, and the build-once
/// artifact closure is enforced. Pins arrive via `mise`; the renderer
/// never invents them.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid pins, context, IR, policy,
/// missing setup/staging, or steps.
pub fn render_workflow_ir_strict(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
) -> Result<String, RenderError> {
    Ok(render_workflow_ir_strict_shared(ir, policy, support, ctx, mise)?.yaml)
}

/// Strict render plus the composite actions duplicated lanes call.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid pins, context, IR, policy,
/// missing setup/staging, steps, or a lane pair whose bodies differ.
pub fn render_workflow_ir_strict_shared(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
) -> Result<RenderedWorkflow, RenderError> {
    render_workflow_ir_strict_shared_with_mode(ir, policy, support, ctx, mise, None)
}

/// Strict render with the effective typed execution mode from configuration or dispatch.
///
/// The mode reaches document emission only after jobs have been finalized, so producer
/// inventory is derived from the exact graph serialized into the workflow.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid pins, context, IR, policy, missing setup/staging,
/// steps, or a lane pair whose bodies differ.
pub fn render_workflow_ir_strict_shared_with_mode(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
    execution_mode: Option<ExecutionMode>,
) -> Result<RenderedWorkflow, RenderError> {
    let jobs = finalize_jobs(ir, policy, support, ctx, mise)?;
    render_merged_with_mode(ir, &jobs, ctx, execution_mode)
}

/// Emit matrix strategy plus the quoted, marked workflow text.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid matrix, lanes, document, marker,
/// size, or steps.
pub fn render_merged(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
) -> Result<RenderedWorkflow, RenderError> {
    render_merged_with_mode(ir, jobs, ctx, None)
}

/// Emit a finalized workflow with an optional explicit execution mode.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid matrix, lanes, document, marker, size, or steps.
pub fn render_merged_with_mode(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    execution_mode: Option<ExecutionMode>,
) -> Result<RenderedWorkflow, RenderError> {
    let matrix = matrix::task_matrix_of(jobs)?;
    let caps = matrix::crate_job_caps(jobs)?;
    let artifact_matrices = artifact_matrix::artifact_matrix_directives(jobs)?;
    let jobs = if matrix.is_some() || !caps.is_empty() {
        matrix::scrub_matrix_marker(jobs)
    } else {
        jobs.clone()
    };
    let jobs = if artifact_matrices.is_empty() {
        jobs
    } else {
        artifact_matrix::scrub_artifact_matrix_markers(&jobs)
    };
    let mbx_jobs = velnor_actions_workflow_cache::mbx_gc_policy::jobs_with_mbx_objects(&jobs);
    let shared = velnor_actions_workflow_document::lane_share::share_lanes(&jobs, ctx)?;
    let mut document =
        document::workflow_to_yaml_with_mode(ir, &shared, ctx, &mbx_jobs, execution_mode)?;
    if let Some((source, max_parallel)) = &matrix {
        matrix::attach_task_matrix(&mut document, source, *max_parallel)?;
    } else {
        matrix::attach_plan_outputs(&mut document)?;
    }
    artifact_matrix::attach_artifact_matrices(&mut document, &artifact_matrices)?;
    matrix::attach_crate_job_caps(&mut document, &caps)?;
    let document = velnor_actions_workflow_tree::yaml::quote_run_values_in_yaml(document);
    let document = velnor_actions_workflow_tree::yaml::share_repeated_run_scalars(document);
    let text = marker::with_marker(&ctx.generator_version, &render_yaml(&document))?;
    velnor_actions_workflow_tree::workflow_size::check_workflow_size(CI_WORKFLOW_PATH, &text)?;
    steps::scan_for_private_subcommands(&text)?;
    let mut files = shared.files;
    if velnor_actions_workflow_cache::tool_seed::any_job_has_seed(&jobs) {
        files.push(velnor_actions_workflow_cache::tool_seed::action_file(
            &ctx.generator_version,
        )?);
    }
    Ok(RenderedWorkflow {
        yaml: text,
        shared: files,
    })
}
