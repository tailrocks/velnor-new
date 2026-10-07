//! Workflow-IR documents and the exact two-file generated tree.
//!
//! Fail-closed gates: exact triggers, concurrency, single runner label,
//! consumer support rejection, and candidate-never-plans invariants.
//! The strict entrypoint additionally mandates Mise setup, staged
//! helpers, and the plan anchor; the legacy entrypoint preserves the
//! previous contract for in-flight callers.

use std::collections::BTreeMap;

use velnor_actions_contract_config::{VelnorSupportWorkflow, WorkflowPolicy};
use velnor_actions_contract_workflow::{CI_WORKFLOW_PATH, Job, WorkflowIr};

use velnor_actions_workflow_steps::{RenderError, setup::MiseSetup, steps};
use velnor_actions_workflow_tree::{marker, yaml::render_yaml};

use velnor_actions_workflow_jobs::{
    RenderContext, closure, final_steps,
    finalize::{finalize_jobs, merged_jobs, validate_final_jobs},
};

use velnor_actions_workflow_document::{document, matrix};

pub use velnor_actions_workflow_document::matrix::{
    COVERED_TASKS_OUTPUT, MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV, MATRIX_OUTPUT_ENV,
    MatrixSource, PLAN_ID_OUTPUT, PLAN_STEP_ID, RUN_KEY_OUTPUT,
};

/// Generated workflow path inside the repository.
///
/// Alias of the contract's [`CI_WORKFLOW_PATH`]: the migration plan
/// ([`velnor_actions_contract_workflow::RequiredCheckMigration`]) and the
/// emitted tree share one source of truth, never retyped mirrors.
pub const WORKFLOW_PATH: &str = CI_WORKFLOW_PATH;

pub use velnor_actions_contract_release::{AGENTS_MD_PATH, CLAUDE_MD_PATH, CLAUDE_MD_TARGET};
pub use velnor_actions_workflow_document::lane_share::RenderedWorkflow;
/// Render one workflow document from IR under a policy gate.
///
/// Inserts the mandated closures (plan, request, task, final) shared by
/// both entrypoints; use [`render_workflow_ir_strict`] for the full
/// fail-closed pass (Mise setup plus staged-helper gates).
///
/// # Errors
///
/// Returns [`RenderError`] for invalid context, IR, policy, or steps.
pub fn render_workflow_ir(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
) -> Result<String, RenderError> {
    Ok(render_workflow_parts(ir, policy, support, ctx)?.yaml)
}

fn render_workflow_parts(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
) -> Result<RenderedWorkflow, RenderError> {
    let mut jobs = merged_jobs(ir, policy, support, ctx)?;
    closure::insert_plan_closure(&mut jobs, ctx)?;
    closure::insert_request_closure(&mut jobs)?;
    closure::insert_task_closure(&mut jobs)?;
    closure::insert_final_closure(&mut jobs)?;
    final_steps::insert_final_fanin(&mut jobs, ctx)?;
    validate_final_jobs(ir, &jobs)?;
    render_merged(ir, &jobs, ctx)
}

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
    let jobs = finalize_jobs(ir, policy, support, ctx, mise)?;
    render_merged(ir, &jobs, ctx)
}

/// Emit matrix strategy plus the quoted, marked workflow text.
fn render_merged(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
) -> Result<RenderedWorkflow, RenderError> {
    let matrix = matrix::task_matrix_of(jobs)?;
    let caps = matrix::crate_job_caps(jobs)?;
    let jobs = if matrix.is_some() || !caps.is_empty() {
        matrix::scrub_matrix_marker(jobs)
    } else {
        jobs.clone()
    };
    let mbx_jobs = velnor_actions_workflow_cache::mbx_gc_policy::jobs_with_mbx_objects(&jobs);
    let shared = velnor_actions_workflow_document::lane_share::share_lanes(&jobs, ctx)?;
    let mut document = document::workflow_to_yaml(ir, &shared, ctx, &mbx_jobs)?;
    if let Some((source, max_parallel)) = &matrix {
        matrix::attach_task_matrix(&mut document, source, *max_parallel)?;
    } else {
        matrix::attach_plan_outputs(&mut document)?;
    }
    matrix::attach_crate_job_caps(&mut document, &caps)?;
    let document = velnor_actions_workflow_tree::yaml::quote_run_values_in_yaml(document);
    let text = marker::with_marker(&ctx.generator_version, &render_yaml(&document))?;
    velnor_actions_workflow_tree::workflow_size::check_workflow_size(WORKFLOW_PATH, &text)?;
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
