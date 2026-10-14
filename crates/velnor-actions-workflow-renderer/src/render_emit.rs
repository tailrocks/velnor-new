//! Emit a merged workflow, including the one oversized-`ci.yml` retry.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, WorkflowIr};

use super::{MatrixSource, RenderContext, RenderedWorkflow, WORKFLOW_PATH, fallback};
use crate::{RenderError, document, matrix, steps};

pub(crate) fn workflow_file_too_large(error: &RenderError) -> bool {
    matches!(
        error,
        RenderError::InvalidWorkflow(problem)
            if problem.starts_with("workflow_too_large:.github/workflows/ci.yml:")
    )
}

pub(crate) fn emit_shared(
    ir: &WorkflowIr,
    shared: &crate::lane_share::LaneShare,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    matrix: Option<&(MatrixSource, u32)>,
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
