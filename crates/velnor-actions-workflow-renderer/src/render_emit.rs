//! Emit a merged workflow, including the one oversized-`ci.yml` retry.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, WorkflowIr};

use super::{MatrixSource, RenderContext, RenderedWorkflow, WORKFLOW_PATH, fallback};
use crate::{
    RenderError, document, matrix, steps,
    workflow_routing::{WorkflowRoutingPolicy, route_workflow_yaml_for_jobs},
};

#[cfg(feature = "test-render-capture")]
#[path = "render_emit_capture.rs"]
mod capture;

pub(crate) struct EmitSettings<'a> {
    pub(crate) matrix: Option<&'a (MatrixSource, u32)>,
    pub(crate) caps: &'a BTreeMap<String, u32>,
    pub(crate) suite_matrices: &'a (
        BTreeMap<String, matrix::suites::SuiteMatrix>,
        BTreeSet<String>,
        BTreeMap<String, matrix::bun::BunLegMatrix>,
    ),
    pub(crate) allow_step_aliases: bool,
    pub(crate) job_workflow_ids: Option<&'a BTreeSet<String>>,
}

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
    settings: &EmitSettings<'_>,
    routing: &WorkflowRoutingPolicy,
) -> Result<RenderedWorkflow, RenderError> {
    #[cfg(feature = "test-render-capture")]
    capture::begin_source_job_workflow_attempt();

    let mbx_jobs = crate::mbx_gc_policy::jobs_with_mbx_objects(jobs);
    let mut document = document::workflow_to_yaml(ir, shared, ctx, &mbx_jobs)?;
    let cache_outputs = crate::cache_plan_outputs::consumed(jobs)?;
    if let Some((source, max_parallel)) = settings.matrix {
        matrix::attach_task_matrix(&mut document, source, *max_parallel, &cache_outputs)?;
    } else {
        matrix::attach_plan_outputs(&mut document, &cache_outputs)?;
    }
    matrix::attach_crate_job_caps(&mut document, settings.caps)?;
    matrix::suites::attach_suite_allow_failure(&mut document, &settings.suite_matrices.1)?;
    matrix::suites::attach_suite_matrices(&mut document, &settings.suite_matrices.0)?;
    matrix::bun::attach_bun_matrices(&mut document, &settings.suite_matrices.2)?;
    document = route_workflow_yaml_for_jobs(WORKFLOW_PATH, document, routing, jobs)?;
    #[cfg(feature = "test-render-capture")]
    // Inspect the final configured lane labels without changing the render result.
    capture::observe_source_job_workflows(ir, jobs, ctx, &document, &mbx_jobs, routing);

    let text = if settings.allow_step_aliases {
        fallback::render_checked_workflow(WORKFLOW_PATH, &document, &ctx.generator_version)?
    } else {
        fallback::render_checked_workflow_before_extraction(
            WORKFLOW_PATH,
            &document,
            &ctx.generator_version,
        )?
    };
    steps::scan_for_private_subcommands(&text)?;
    let mut shared_files = crate::render_cache_files::with_runtime_identity_and_cache_tar_files(
        shared.files.clone(),
        jobs,
        &ctx.generator_version,
        &ctx.staged_binary,
        &ctx.report_helper_version,
        &ctx.tool_pins,
    )?;
    let relocations = document::render_job_workflow_relocations_with_routing(
        ir, shared, ctx, &mbx_jobs, routing,
    )?;
    for relocation in &relocations {
        if shared_files
            .iter()
            .any(|file| file.path == relocation.expected_callee_file.path)
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "job_workflow_extract:duplicate_callee_path:{}",
                relocation.expected_callee_file.path
            )));
        }
        shared_files.push(relocation.expected_callee_file.clone());
    }
    if let Some(selected_source_ids) = settings.job_workflow_ids {
        crate::workflow_routing::validate_relocated_job_workflows_with_selection(
            &document,
            &shared_files,
            selected_source_ids,
            &relocations,
        )?;
    } else {
        crate::workflow_routing::validate_relocated_job_workflows(
            &document,
            &shared_files,
            &relocations,
        )?;
    }
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
