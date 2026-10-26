use std::collections::BTreeMap;

use velnor_actions_contract::{Job, VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy};

use crate::{
    RenderError, cache_p08, closure, commands, final_steps, guard, marker, matrix, msrv,
    preseed_closure, steps, support, workflow_policy,
};

#[path = "render_execution_boundaries.rs"]
mod execution_boundaries;

type JobMap = BTreeMap<String, Job>;

#[path = "render_emit.rs"]
mod emit;

#[path = "render_fallback.rs"]
pub(crate) mod fallback;

#[cfg(feature = "test-render-capture")]
#[path = "test_render_capture.rs"]
pub mod test_render_capture;

#[path = "render_action_pins.rs"]
mod action_pins_impl;
pub use action_pins_impl::action_pins;

#[path = "validator_tools.rs"]
mod validator_tools;

pub const ALINT_BINARY_VERSION: &str = "v0.20.1";

#[path = "render_context.rs"]
mod context;
pub use self::context::*;

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
    closure::insert_plan_closure(&mut jobs, ctx, policy)?;
    closure::insert_request_closure(&mut jobs)?;
    closure::insert_task_closure(&mut jobs)?;
    closure::insert_final_closure(&mut jobs)?;
    final_steps::insert_final_fanin(&mut jobs, ctx)?;
    validate_final_jobs(ir, &mut jobs, policy)?;
    render_merged(ir, &jobs, ctx, policy)
}

pub fn render_workflow_ir_strict(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
) -> Result<String, RenderError> {
    Ok(render_workflow_ir_strict_shared(ir, policy, support, ctx, mise)?.yaml)
}

pub fn render_workflow_ir_strict_shared(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
) -> Result<RenderedWorkflow, RenderError> {
    let jobs = finalize_jobs(ir, policy, support, ctx, mise)?;
    render_merged(ir, &jobs, ctx, policy)
}

pub fn finalize_jobs(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
) -> Result<BTreeMap<String, Job>, RenderError> {
    mise.validate()?;
    let mut jobs = merged_jobs(ir, policy, support, ctx)?;
    for (id, job) in &mut jobs {
        if ctx
            .workflow_tasks
            .iter()
            .any(|task| task.owns_job_id(id) && !task.uses_pinned_tools())
        {
            closure::check_internal_staged(id, job, ctx.preseed)?;
            continue;
        }
        let always = id == PLAN_JOB_ID || id == TASK_JOB_ID || job.check_runner.is_some();
        let target = job
            .check_runner
            .as_ref()
            .map(|runner| runner.platform.target())
            .or_else(|| crate::runs_on::target_for_runner(&job.runs_on))
            .ok_or_else(|| {
                RenderError::InvalidWorkflow(format!("tools_cache_unsupported_target:{id}"))
            })?;
        let default_target = crate::runs_on::target_for_runner(&ctx.runs_on);
        let setup = if job.check_runner.is_some() || default_target != Some(target) {
            mise.for_target(target)?
        } else {
            mise.clone()
        };
        cache_p08::ensure_tools_cache_v2(
            id,
            job,
            &setup,
            always,
            target,
            &ctx.checkout_uses,
            policy == WorkflowPolicy::VelnorRepositoryV1 && id == PLAN_JOB_ID,
        )?;
        cache_p08::check_no_legacy_rust_cache(id, job)?;
        cache_p08::check_mbx_before_fetch(id, job)?;
        closure::check_internal_staged(id, job, ctx.preseed)?;
    }
    cache_p08::elect_cache_writers_with_policy(&mut jobs, ctx.pull_request_cache_policy)?;
    closure::check_plan_anchor(&jobs)?;
    preseed_closure::check_preseed_closure(&jobs, ctx.preseed)?;
    closure::insert_plan_closure(&mut jobs, ctx, policy)?;
    closure::insert_request_closure(&mut jobs)?;
    closure::insert_task_closure(&mut jobs)?;
    closure::insert_final_closure(&mut jobs)?;
    final_steps::insert_final_fanin(&mut jobs, ctx)?;
    crate::dispatch_cache_boundary::suppress_unvalidated_cache_access(&mut jobs);
    validate_final_jobs(ir, &mut jobs, policy)?;
    crate::cache_doctor::check(&jobs)?;
    Ok(jobs)
}

fn validate_final_jobs(
    ir: &WorkflowIr,
    jobs: &mut JobMap,
    policy: WorkflowPolicy,
) -> Result<(), RenderError> {
    crate::cache_steps::append_bundle_postludes(jobs)?;
    crate::cache_steps::append_workspace_cleanups(jobs)?;
    execution_boundaries::validate_required_verdict(jobs)?;
    execution_boundaries::validate_trusted_plan(jobs, policy)?;
    let mut finalized = ir.clone();
    finalized.jobs.clone_from(jobs);
    finalized.validate().map_err(RenderError::Contract)
}

fn merged_jobs(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
) -> Result<BTreeMap<String, Job>, RenderError> {
    ctx.validate()?;
    if ctx.preseed && policy != WorkflowPolicy::VelnorRepositoryV1 {
        return Err(RenderError::PolicyRejected {
            policy: "consumer-v1".to_owned(),
            problem: "preseed_requires_velnor_policy".to_owned(),
        });
    }
    ir.validate().map_err(RenderError::Contract)?;
    workflow_policy::check_triggers(&ir.triggers)?;
    workflow_policy::check_concurrency(&ir.concurrency)?;
    workflow_policy::check_single_label(ir, &ctx.runs_on, &ctx.workflow_tasks)?;
    let mut jobs = ir.jobs.clone();
    match policy {
        WorkflowPolicy::ConsumerV1 => support::merge_consumer_verify(&mut jobs, support, ctx)?,
        WorkflowPolicy::VelnorRepositoryV1 => {
            support::merge_support_jobs(&mut jobs, support, ctx, "velnor-repository-v1")?;
        }
    }
    let workflow_task_ids =
        crate::verification_jobs::workflow_task_jobs::validate_workflow_task_jobs(
            &jobs,
            &ctx.workflow_tasks,
            &ctx.checkout_uses,
        )?;
    crate::verification_jobs::workflow_task_jobs::extend_required_needs(
        &mut jobs,
        &workflow_task_ids,
    )?;
    crate::required_gate::insert_required_gate(&mut jobs)?;
    msrv::check_no_msrv(&jobs)?;
    support::check_candidate_invariants(&jobs)?;
    support::check_final_gate(&jobs)?;
    crate::required_gate::check_required_gate(&jobs)?;
    support::check_token_hygiene(&jobs, &ctx.report_helper_version)?;
    Ok(jobs)
}

fn render_merged(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    policy: WorkflowPolicy,
) -> Result<RenderedWorkflow, RenderError> {
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
    let mut shared = crate::lane_share::share_lanes(&jobs, ctx)?;
    shared.files.extend(task_files);
    execution_boundaries::validate_required_verdict(&shared.jobs)?;
    execution_boundaries::validate_trusted_plan(&shared.jobs, policy)?;
    match emit::emit_shared(
        ir,
        &shared,
        &jobs,
        ctx,
        matrix.as_ref(),
        &caps,
        &suite_matrices,
    ) {
        Ok(workflow) => Ok(workflow),
        Err(error) if emit::workflow_file_too_large(&error) => {
            let extracted =
                crate::crate_obligations::extract_obligation_composites(&mut shared, ctx)?;
            if extracted == 0 {
                return Err(error);
            }
            execution_boundaries::validate_required_verdict(&shared.jobs)?;
            execution_boundaries::validate_trusted_plan(&shared.jobs, policy)?;
            emit::emit_shared(
                ir,
                &shared,
                &jobs,
                ctx,
                matrix.as_ref(),
                &caps,
                &suite_matrices,
            )
        }
        Err(error) => Err(error),
    }
}
