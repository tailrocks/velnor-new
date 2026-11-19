// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, WorkflowIr};

use super::{MatrixSource, PLAN_JOB_ID, RenderContext, RenderedWorkflow, WORKFLOW_PATH, fallback};
use crate::{RenderError, document, matrix, steps};

pub(crate) fn workflow_file_too_large(error: &RenderError) -> bool {
    matches!(
        error,
        RenderError::InvalidWorkflow(problem)
            if problem.starts_with("workflow_too_large:.github/workflows/ci.yml:")
    )
}

#[derive(Clone, Copy)]
pub(crate) struct EmitScope<'a> {
    pub(crate) jobs: &'a BTreeMap<String, Job>,
    pub(crate) all_jobs: &'a BTreeMap<String, Job>,
}

pub(crate) fn emit_shared(
    ir: &WorkflowIr,
    shared: &crate::lane_share::LaneShare,
    scope: &EmitScope<'_>,
    ctx: &RenderContext,
    matrix: Option<&(MatrixSource, u32)>,
    caps: &BTreeMap<String, u32>,
    suite_matrices: &(
        BTreeMap<String, matrix::suites::SuiteMatrix>,
        BTreeSet<String>,
        BTreeMap<String, matrix::bun::BunLegMatrix>,
    ),
) -> Result<RenderedWorkflow, RenderError> {
    let EmitScope { jobs, all_jobs } = *scope;
    let mbx_jobs = crate::mbx_gc_policy::jobs_with_mbx_objects(jobs);
    let mut document = document::workflow_to_yaml(ir, shared, ctx, &mbx_jobs)?;
    let cache_outputs = crate::cache_plan_outputs::consumed(jobs, true)?;
    if let Some((source, max_parallel)) = matrix {
        matrix::attach_task_matrix(&mut document, source, *max_parallel, &cache_outputs)?;
    } else {
        matrix::attach_plan_outputs(&mut document, &cache_outputs)?;
    }
    matrix::attach_crate_job_caps(&mut document, caps)?;
    matrix::suites::attach_suite_allow_failure(&mut document, &suite_matrices.1)?;
    matrix::suites::attach_suite_matrices(&mut document, &suite_matrices.0)?;
    matrix::bun::attach_bun_matrices(&mut document, &suite_matrices.2)?;
    let source_qualification_plan = source_qualification_plan(&document, ctx)?;
    crate::pr_ci::apply(
        &mut document,
        ctx.pr_ci,
        crate::pr_ci::PrCiShape {
            quick: &[&[PLAN_JOB_ID], crate::pr_ci::validator_job_ids().as_slice()].concat(),
            claim: true,
            merge_group: true,
        },
    )?;
    let text = fallback::render_checked_workflow(WORKFLOW_PATH, &document, &ctx.generator_version)?;
    steps::scan_for_private_subcommands(&text)?;
    let shared_files = crate::render_cache_files::with_runtime_identity_files(
        shared.files.clone(),
        all_jobs,
        &ctx.generator_version,
        &ctx.tool_pins,
    )?;
    if crate::tool_seed::any_job_has_seed(all_jobs)? {
        return Err(RenderError::InvalidWorkflow(
            "tool_seed_requires_tools_prelude".to_owned(),
        ));
    }
    Ok(RenderedWorkflow {
        yaml: text,
        shared: shared_files,
        schema2_files: Vec::new(),
        source_qualification_plan,
    })
}

fn source_qualification_plan(
    document: &crate::yaml::Yaml,
    ctx: &RenderContext,
) -> Result<Option<crate::source_qualification_plan::SourceQualificationPlan>, RenderError> {
    let crate::yaml::Yaml::Map(root) = document else {
        return Err(RenderError::InvalidWorkflow(
            "source_qualification_plan_document".to_owned(),
        ));
    };
    let Some((_, crate::yaml::Yaml::Map(jobs))) = root.iter().find(|(key, _)| key == "jobs") else {
        return Err(RenderError::InvalidWorkflow(
            "source_qualification_plan_jobs".to_owned(),
        ));
    };
    let Some(mut job) = jobs
        .iter()
        .find(|(id, _)| id == "plan")
        .map(|(_, job)| job.clone())
    else {
        return Ok(None);
    };
    if let crate::yaml::Yaml::Map(fields) = &mut job {
        fields.retain(|(key, _)| key != "if" && key != "defaults");
        if let Some((_, runs_on)) = fields.iter_mut().find(|(key, _)| key == "runs-on") {
            *runs_on = crate::yaml::Yaml::str(ctx.runs_on.clone());
        }
    }
    Ok(Some(
        crate::source_qualification_plan::SourceQualificationPlan {
            job,
            cli_routes: Vec::new(),
            non_inputs: ctx.non_inputs.clone(),
            controller_path: ctx.controller.path().to_owned(),
            controller_version: ctx.controller.report_helper_version().to_owned(),
        },
    ))
}
