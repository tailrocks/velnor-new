use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, WorkflowIr};

use super::{RenderContext, RenderError, RenderedWorkflow, emit};
use crate::matrix;

pub(super) fn render_merged_with_routing(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    routing: &crate::workflow_routing::WorkflowRoutingPolicy,
    job_workflow_ids: Option<&BTreeSet<String>>,
) -> Result<RenderedWorkflow, RenderError> {
    let routing = (*routing).clone().with_verified_workflow_tasks(
        ir,
        &ctx.workflow_tasks,
        &ctx.checkout_uses,
    )?;
    let matrix = matrix::task_matrix_of(jobs)?;
    let caps = matrix::crate_job_caps(jobs)?;
    let suite_matrices = (
        matrix::suites::suite_matrices_of(jobs)?,
        matrix::suites::suite_allow_failure_jobs_of(jobs),
        matrix::bun::bun_matrices_of(jobs)?,
    );
    let jobs = if matrix.is_some()
        || !caps.is_empty()
        || !suite_matrices.0.is_empty()
        || !suite_matrices.1.is_empty()
        || !suite_matrices.2.is_empty()
    {
        matrix::scrub_matrix_marker(jobs)
    } else {
        jobs.clone()
    };
    let (jobs, task_files) = crate::task_wrapper::factor_obligation_steps(
        &jobs,
        &ctx.checkout_uses,
        &ctx.generator_version,
        &ctx.report_helper_version,
        &ctx.workflow_tasks,
        ctx.scale_set_selector.as_ref(),
    )?;
    let mut shared = crate::lane_share::share_lanes_with_selection(&jobs, ctx, job_workflow_ids)?;
    shared.files.extend(task_files);
    let first_emit = emit::EmitSettings {
        matrix: matrix.as_ref(),
        caps: &caps,
        suite_matrices: &suite_matrices,
        allow_step_aliases: false,
        job_workflow_ids,
    };
    match emit::emit_shared(ir, &shared, &jobs, ctx, &first_emit, &routing) {
        Ok(workflow) => Ok(workflow),
        Err(error) if emit::workflow_file_too_large(&error) => {
            let extracted =
                crate::crate_obligations::extract_obligation_composites(&mut shared, ctx)?;
            let retry_emit = emit::EmitSettings {
                allow_step_aliases: true,
                ..first_emit
            };
            match emit::emit_shared(ir, &shared, &jobs, ctx, &retry_emit, &routing) {
                Ok(workflow) => Ok(workflow),
                Err(second_error) if emit::workflow_file_too_large(&second_error) => {
                    let shared_ids = shared.calls.keys().cloned().collect();
                    let cache_files = crate::qualification_cache_composites::extract(
                        &mut shared.jobs,
                        &shared_ids,
                        ctx,
                    )?;
                    if cache_files.is_empty() {
                        return Err(if extracted == 0 { error } else { second_error });
                    }
                    shared.files.extend(cache_files);
                    emit::emit_shared(ir, &shared, &jobs, ctx, &retry_emit, &routing)
                }
                Err(second_error) => Err(second_error),
            }
        }
        Err(error) => Err(error),
    }
}
