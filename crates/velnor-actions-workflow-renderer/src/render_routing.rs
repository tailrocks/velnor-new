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
    let jobs = super::finalize_jobs_with_routing(ir, policy, support, ctx, mise, routing)?;
    super::render_merged(ir, &jobs, ctx)
}
