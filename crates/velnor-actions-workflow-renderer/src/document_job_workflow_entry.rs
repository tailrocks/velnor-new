use std::collections::BTreeSet;

use velnor_actions_contract::WorkflowIr;

use super::{LaneShare, RenderContext, RenderError};

#[cfg(test)]
pub(crate) fn render_job_workflow_relocations(
    ir: &WorkflowIr,
    shared: &LaneShare,
    ctx: &RenderContext,
    mbx_jobs: &BTreeSet<String>,
) -> Result<Vec<crate::lane_share::job_workflow_extract::JobWorkflowRelocation>, RenderError> {
    render_job_workflow_relocations_with_routing(
        ir,
        shared,
        ctx,
        mbx_jobs,
        &crate::workflow_routing::WorkflowRoutingPolicy::hosted_only(),
    )
}

/// Render source-bound reusable-workflow snapshots for generated child jobs.
pub(crate) fn render_job_workflow_relocations_with_routing(
    ir: &WorkflowIr,
    shared: &LaneShare,
    ctx: &RenderContext,
    mbx_jobs: &BTreeSet<String>,
    routing: &crate::workflow_routing::WorkflowRoutingPolicy,
) -> Result<Vec<crate::lane_share::job_workflow_extract::JobWorkflowRelocation>, RenderError> {
    let workflow_env =
        crate::document_env::workflow_env(!ctx.workflow_tasks.is_empty(), ctx.mise_quiet);
    shared
        .job_workflows
        .values()
        .map(|prepared| {
            super::job_workflow::relocation(prepared, ir, ctx, mbx_jobs, &workflow_env, routing)
        })
        .collect()
}

#[cfg(feature = "test-render-capture")]
pub(crate) fn render_job_workflow_relocations_for_capture(
    ir: &WorkflowIr,
    shared: &LaneShare,
    ctx: &RenderContext,
    mbx_jobs: &BTreeSet<String>,
    routing: &crate::workflow_routing::WorkflowRoutingPolicy,
) -> Result<Vec<crate::lane_share::job_workflow_extract::JobWorkflowRelocation>, RenderError> {
    render_job_workflow_relocations_with_routing(ir, shared, ctx, mbx_jobs, routing)
}
