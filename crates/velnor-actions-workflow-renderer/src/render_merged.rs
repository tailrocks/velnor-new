//! Emit matrix strategy plus the quoted, marked workflow text.
//!
//! A file over GitHub's 500 KB cap is rendered again after each inlined
//! crate obligation list moves into its own composite. The second miss
//! still fails closed, and nothing is truncated.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, WorkflowIr};

use crate::{RenderError, document, matrix, steps};

use super::{RenderContext, RenderedWorkflow, WORKFLOW_PATH, fallback, task_wrapper};

pub(super) fn render_merged(
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
    let (jobs, task_files) = task_wrapper::factor_obligation_steps(
        &jobs,
        &ctx.checkout_uses,
        &ctx.generator_version,
        &ctx.report_helper_version,
        &ctx.workflow_tasks,
        ctx.scale_set_selector.as_ref(),
    )?;
    let mut shared = crate::lane_share::share_lanes(&jobs, ctx)?;
    shared.files.extend(task_files);
    match emit_shared(ir, &shared, &jobs, ctx, matrix.as_ref(), &caps) {
        Ok(workflow) => Ok(workflow),
        Err(error) if workflow_file_too_large(&error) => {
            let extracted =
                crate::crate_obligations::extract_obligation_composites(&mut shared, ctx)?;
            if extracted == 0 {
                return Err(error);
            }
            emit_shared(ir, &shared, &jobs, ctx, matrix.as_ref(), &caps)
        }
        Err(error) => Err(error),
    }
}

fn workflow_file_too_large(error: &RenderError) -> bool {
    matches!(
        error,
        RenderError::InvalidWorkflow(problem)
            if problem.starts_with("workflow_too_large:.github/workflows/ci.yml:")
    )
}

fn emit_shared(
    ir: &WorkflowIr,
    shared: &crate::lane_share::LaneShare,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    matrix: Option<&(matrix::MatrixSource, u32)>,
    caps: &BTreeMap<String, u32>,
) -> Result<RenderedWorkflow, RenderError> {
    let mbx_jobs = crate::mbx_gc_policy::jobs_with_mbx_objects(jobs);
    let mut document = document::workflow_to_yaml(ir, shared, ctx, &mbx_jobs)?;
    if let Some((source, max_parallel)) = matrix {
        matrix::attach_task_matrix(&mut document, source, *max_parallel)?;
    } else {
        matrix::attach_plan_outputs(&mut document)?;
    }
    matrix::attach_crate_job_caps(&mut document, caps)?;
    let text = fallback::render_checked_workflow(WORKFLOW_PATH, &document, &ctx.generator_version)?;
    steps::scan_for_private_subcommands(&text)?;
    let shared_files = crate::render_cache_files::with_runtime_identity_files(
        shared.files.clone(),
        jobs,
        &ctx.generator_version,
    )?;
    if crate::tool_seed::any_job_has_seed(jobs)? {
        return Err(RenderError::InvalidWorkflow(
            "tool_seed_requires_tools_prelude".to_owned(),
        ));
    }
    Ok(RenderedWorkflow {
        yaml: text,
        shared: shared_files,
    })
}
