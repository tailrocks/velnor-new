use std::collections::BTreeMap;

use velnor_actions_contract::{
    Job, ReleaseTarget, RunsOn, SCALE_SET_NAME, VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};

use crate::render::{PLAN_JOB_ID, RenderContext, TASK_JOB_ID, merged_jobs, validate_final_jobs};
use crate::{RenderError, cache_p08, closure, final_steps, preseed_closure, setup::MiseSetup};

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
            crate::tool_seed::reject_orphan_seed(id, job)?;
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

fn target_for_runner(label: &str) -> Option<&'static str> {
    match RunsOn::parse(label).ok()? {
        RunsOn::Hosted(label) => ReleaseTarget::for_runner_label(&label).map(ReleaseTarget::triple),
        RunsOn::ScaleSet(selector) if selector.name() == SCALE_SET_NAME => {
            Some(ReleaseTarget::LinuxX86_64.triple())
        }
        RunsOn::ScaleSet(_) => None,
    }
}
