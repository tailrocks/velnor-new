//! Policy merge plus setup insertion, closures, and the final fan-in.

use std::collections::BTreeMap;

use velnor_actions_contract_config::{VelnorSupportWorkflow, WorkflowPolicy};
use velnor_actions_contract_workflow::{Job, WorkflowIr};
use velnor_actions_workflow_cache::{cache_p08, tool_seed};
use velnor_actions_workflow_steps::{RenderError, setup::MiseSetup};
use velnor_actions_workflow_tree::runs_on::target_for_runner;

use crate::{
    closure,
    context::{PLAN_JOB_ID, RenderContext, TASK_JOB_ID},
    final_steps, msrv, preseed_closure, support, verification_jobs, workflow_policy,
};

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
        if ctx
            .verification_tasks
            .iter()
            .any(|task| task.owns_job_id(id))
        {
            tool_seed::reject_orphan_seed(id, job)?;
            closure::check_internal_staged(id, job, ctx.preseed)?;
            continue;
        }
        let always = id == PLAN_JOB_ID || id == TASK_JOB_ID || job.check_runner.is_some();
        let target = job
            .check_runner
            .as_ref()
            .map(|runner| runner.platform.target())
            .or_else(|| target_for_runner(&job.runs_on))
            .ok_or_else(|| {
                RenderError::InvalidWorkflow(format!("tools_cache_unsupported_target:{id}"))
            })?;
        let setup = if job.check_runner.is_some() {
            mise.for_target(target)?
        } else {
            mise.clone()
        };
        cache_p08::ensure_setup_p08(id, job, &setup, always, target, &ctx.checkout_uses)?;
        cache_p08::check_no_rust_cache_with_mbx(id, job)?;
        cache_p08::check_mbx_before_fetch(id, job)?;
        closure::check_internal_staged(id, job, ctx.preseed)?;
    }
    // Writer election needs every setup inserted: one saver per key.
    cache_p08::elect_mise_cache_writers(&mut jobs)?;
    // Provider election needs every restore inserted: one saver per key.
    cache_p08::elect_tofu_provider_savers(&mut jobs)?;
    closure::check_plan_anchor(&jobs)?;
    preseed_closure::check_preseed_closure(&jobs, ctx.preseed)?;
    closure::insert_plan_closure(&mut jobs, ctx)?;
    closure::insert_request_closure(&mut jobs)?;
    closure::insert_task_closure(&mut jobs)?;
    closure::insert_final_closure(&mut jobs)?;
    final_steps::insert_final_fanin(&mut jobs, ctx)?;
    validate_final_jobs(ir, &jobs)?;
    Ok(jobs)
}

/// Revalidate each complete job after policy merge and all internal expansion.
///
/// Shared by the strict entry ([`finalize_jobs`]) and the legacy entry,
/// which inserts closures over [`merged_jobs`] without setup insertion.
///
/// # Errors
///
/// Returns [`RenderError`] when the completed jobs violate the IR contract.
pub fn validate_final_jobs(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    let mut finalized = ir.clone();
    finalized.jobs.clone_from(jobs);
    finalized.validate().map_err(RenderError::Contract)
}

/// Validate context/IR plus policy merge and support invariants.
///
/// Shared by both entrypoints; [`finalize_jobs`] adds setup insertion,
/// closures, and the final fan-in on top.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid context, IR, policy, or support.
pub fn merged_jobs(
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
    workflow_policy::check_single_label(ir, &ctx.runs_on, &ctx.verification_tasks)?;
    let mut jobs = ir.jobs.clone();
    support::merge_support_jobs(&mut jobs, support, ctx, policy)?;
    let verification_ids = verification_jobs::validate_verification_jobs(
        &jobs,
        &ctx.verification_tasks,
        &ctx.checkout_uses,
    )?;
    verification_jobs::extend_required_needs(&mut jobs, &verification_ids)?;
    msrv::check_no_msrv(&jobs)?;
    support::check_candidate_invariants(&jobs)?;
    support::check_final_gate(&jobs)?;
    support::check_token_hygiene(&jobs)?;
    Ok(jobs)
}
