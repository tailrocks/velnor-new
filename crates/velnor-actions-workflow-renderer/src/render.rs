//! Workflow-IR documents and the exact two-file generated tree.
//!
//! Fail-closed gates cover triggers, concurrency, runner, consumer support,
//! and candidate-planning invariants. Strict rendering also mandates Mise,
//! staged helpers, and the plan anchor; legacy preserves the prior contract.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    Job, PLAN_JOB_ID as CONTRACT_PLAN_JOB_ID, VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};

use crate::{
    RenderError, cache_p08, closure, commands, document, final_steps, guard, marker, matrix, msrv,
    preseed_closure, steps, support, workflow_policy,
};

type JobMap = BTreeMap<String, Job>;

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

#[path = "task_wrapper.rs"]
mod task_wrapper;

#[path = "render_context.rs"]
mod context;
pub use self::context::*;

/// Render one workflow document from IR under a policy gate.
///
/// Inserts the mandated closures shared by both entrypoints; use
/// [`render_workflow_ir_strict`] for setup and staged-helper gates.
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
    validate_final_jobs(ir, &mut jobs)?;
    render_merged(ir, &jobs, ctx)
}

/// Strict render: setup insertion plus staged-helper and anchor gates.
///
/// Every `mise`-invoking job (plus plan/task unconditionally) gains a
/// preceding pinned setup step; internal steps without a preceding
/// Acquire step and anchorless plan jobs fail closed. In pre-seed mode
/// the fixed pre-seed stage step stages instead, and the build-once
/// artifact closure is enforced. Pins arrive via `mise`; the renderer
/// never invents them.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid pins, context, IR, policy,
/// missing setup/staging, or steps.
pub fn render_workflow_ir_strict(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
) -> Result<String, RenderError> {
    Ok(render_workflow_ir_strict_shared(ir, policy, support, ctx, mise)?.yaml)
}

/// Strict render plus the composite actions duplicated lanes call.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid pins, context, IR, policy,
/// missing setup/staging, steps, or a lane pair whose bodies differ.
pub fn render_workflow_ir_strict_shared(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
) -> Result<RenderedWorkflow, RenderError> {
    let jobs = finalize_jobs(ir, policy, support, ctx, mise)?;
    render_merged(ir, &jobs, ctx)
}

/// Finalize jobs exactly as `generate` writes them: policy merge, setup
/// insertion, then every closure and the final fan-in.
///
/// `plan` lists these jobs (validators included) so its job table,
/// step counts, and action pins match the written YAML by construction
/// instead of echoing the pre-merge IR.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid pins, context, IR, policy,
/// missing setup/staging, or steps.
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
        if ctx.workflow_tasks.iter().any(|task| task.owns_job_id(id)) {
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
        let setup = if job.check_runner.is_some() {
            mise.for_target(target)?
        } else {
            mise.clone()
        };
        cache_p08::ensure_tools_cache_v2(id, job, &setup, always, target, &ctx.checkout_uses)?;
        cache_p08::check_no_legacy_rust_cache(id, job)?;
        cache_p08::check_mbx_before_fetch(id, job)?;
        closure::check_internal_staged(id, job, ctx.preseed)?;
    }
    // Both cache families are validated globally before either gets a save.
    cache_p08::elect_cache_writers(&mut jobs)?;
    closure::check_plan_anchor(&jobs)?;
    preseed_closure::check_preseed_closure(&jobs, ctx.preseed)?;
    closure::insert_plan_closure(&mut jobs, ctx)?;
    closure::insert_request_closure(&mut jobs)?;
    closure::insert_task_closure(&mut jobs)?;
    closure::insert_final_closure(&mut jobs)?;
    final_steps::insert_final_fanin(&mut jobs, ctx)?;
    crate::dispatch_cache_boundary::suppress_unvalidated_cache_access(&mut jobs);
    validate_final_jobs(ir, &mut jobs)?;
    Ok(jobs)
}

/// Validate every finalized job after policy merging and internal expansion.
fn validate_final_jobs(ir: &WorkflowIr, jobs: &mut JobMap) -> Result<(), RenderError> {
    crate::cache_steps::append_workspace_cleanups(jobs)?;
    let mut finalized = ir.clone();
    finalized.jobs.clone_from(jobs);
    finalized.validate().map_err(RenderError::Contract)
}

/// Validate context/IR plus policy merge and support invariants.
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
    msrv::check_no_msrv(&jobs)?;
    support::check_candidate_invariants(&jobs)?;
    support::check_final_gate(&jobs)?;
    support::check_token_hygiene(&jobs)?;
    Ok(jobs)
}

/// Emit matrix strategy plus the quoted, marked workflow text.
fn render_merged(
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
    let mbx_jobs = crate::mbx_gc_policy::jobs_with_mbx_objects(&jobs);
    let mut shared = crate::lane_share::share_lanes(&jobs, ctx)?;
    shared.files.extend(task_files);
    let mut document = document::workflow_to_yaml(ir, &shared, ctx, &mbx_jobs)?;
    if let Some((source, max_parallel)) = &matrix {
        matrix::attach_task_matrix(&mut document, source, *max_parallel)?;
    } else {
        matrix::attach_plan_outputs(&mut document)?;
    }
    matrix::attach_crate_job_caps(&mut document, &caps)?;
    #[cfg(feature = "test-render-capture")]
    test_render_capture::record_shared(crate::render_cache_files::with_runtime_identity_files(
        shared.files.clone(),
        &jobs,
        &ctx.generator_version,
    )?);
    let text = fallback::render_checked_workflow(WORKFLOW_PATH, &document, &ctx.generator_version)?;
    steps::scan_for_private_subcommands(&text)?;
    let shared_files = crate::render_cache_files::with_runtime_identity_files(
        shared.files,
        &jobs,
        &ctx.generator_version,
    )?;
    if crate::tool_seed::any_job_has_seed(&jobs)? {
        return Err(RenderError::InvalidWorkflow(
            "tool_seed_requires_tools_prelude".to_owned(),
        ));
    }
    Ok(RenderedWorkflow {
        yaml: text,
        shared: shared_files,
    })
}
