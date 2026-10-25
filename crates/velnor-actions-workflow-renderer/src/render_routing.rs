use std::collections::BTreeSet;

use velnor_actions_contract::{VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy};

use crate::{
    RenderError,
    render::{MiseSetup, RenderContext, RenderedWorkflow},
    workflow_routing::WorkflowRoutingPolicy,
};

/// Apply runner placement after policy jobs join the typed job map, then render.
///
/// # Errors
///
/// The function gives an error when rendering or runner placement fails.
pub fn render_workflow_ir_strict_shared_with_routing(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
    routing: &WorkflowRoutingPolicy,
) -> Result<RenderedWorkflow, RenderError> {
    render_workflow_ir_strict_shared_with_routing_and_job_workflows(
        ir, policy, support, ctx, mise, routing, None,
    )
}

/// Strictly render a workflow and the explicitly selected source-derived job workflows.
///
/// Passing `None` preserves the existing prototype behavior. `Some(empty)` deliberately
/// selects no job workflows and does not use the prototype fallback.
#[doc(hidden)]
pub fn render_workflow_ir_strict_shared_with_routing_and_job_workflows(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
    routing: &WorkflowRoutingPolicy,
    job_workflow_ids: Option<&BTreeSet<String>>,
) -> Result<RenderedWorkflow, RenderError> {
    let jobs = super::finalize_jobs(ir, policy, support, ctx, mise, routing)?;
    super::render_merged_with_routing_and_job_workflows(ir, &jobs, ctx, routing, job_workflow_ids)
}
