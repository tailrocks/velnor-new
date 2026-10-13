//! Workflow-IR documents and the exact two-file generated tree.
//!
//! Fail-closed gates: exact triggers, concurrency, single runner label,
//! consumer support rejection, and candidate-never-plans invariants.
//! The legacy entrypoint preserves the previous contract for in-flight
//! callers; strict emission lives in the render-strict crate.

use velnor_actions_contract_config::{VelnorSupportWorkflow, WorkflowPolicy};
use velnor_actions_contract_workflow::{CI_WORKFLOW_PATH, WorkflowIr};

use velnor_actions_workflow_jobs::{
    RenderContext, closure, final_steps,
    finalize::{merged_jobs, validate_final_jobs},
};
use velnor_actions_workflow_steps::RenderError;

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
/// Inserts the mandated closures (plan, request, task, final); merged
/// emission is shared with the strict entrypoint.
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
    velnor_actions_workflow_render_strict::render_merged(ir, &jobs, ctx)
}
