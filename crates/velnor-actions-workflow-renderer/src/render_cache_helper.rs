use velnor_actions_contract::Job;

use super::{RenderContext, RenderError, cache_p08};

/// If the orchestrator supplied verified helper steps for a runner label, put
/// those steps before cache use for that runner.
pub(super) fn insert_verified_cache_helper(
    job_id: &str,
    job: &mut Job,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    let actual = job
        .steps
        .iter()
        .filter(|step| is_verified_cache_helper_step(step))
        .cloned()
        .collect::<Vec<_>>();
    let Some(helper_steps) = ctx.verified_cache_helpers.get(&job.runs_on) else {
        if ctx.require_verified_cache_helper && !actual.is_empty() {
            return Err(RenderError::InvalidWorkflow(format!(
                "cache_helper_unmapped:{job_id}"
            )));
        }
        return Ok(());
    };
    if helper_steps.is_empty()
        || helper_steps
            .iter()
            .any(|step| !is_verified_cache_helper_step(step))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "cache_helper_map_malformed:{job_id}"
        )));
    }
    if !actual.is_empty() {
        if actual.as_slice() != helper_steps.as_slice() {
            return Err(RenderError::InvalidWorkflow(format!(
                "cache_helper_map_mismatch:{job_id}"
            )));
        }
        if actual.len() == 1
            && actual[0].role == Some(velnor_actions_contract::StepRole::AcquireVelnor)
        {
            cache_p08::move_acquire_after_checkout(job, &ctx.checkout_uses);
        }
        return Ok(());
    }
    let Some(checkout_at) = job
        .steps
        .iter()
        .position(|step| step.role == Some(velnor_actions_contract::StepRole::Checkout))
    else {
        return Err(RenderError::InvalidWorkflow(format!(
            "cache_helper_checkout_missing:{job_id}"
        )));
    };
    for (offset, step) in helper_steps.iter().cloned().enumerate() {
        job.steps.insert(checkout_at + 1 + offset, step);
    }
    Ok(())
}

fn is_verified_cache_helper_step(step: &velnor_actions_contract::Step) -> bool {
    matches!(
        step.role,
        Some(
            velnor_actions_contract::StepRole::AcquireVelnor
                | velnor_actions_contract::StepRole::PreseedDownload
                | velnor_actions_contract::StepRole::PreseedVerifyManifest
                | velnor_actions_contract::StepRole::PreseedStage
        )
    )
}
