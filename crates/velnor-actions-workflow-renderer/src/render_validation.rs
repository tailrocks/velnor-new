use std::collections::BTreeMap;

use velnor_actions_contract::{Job, WorkflowIr};

use crate::{RenderError, render::PLAN_JOB_ID};

/// Skip an affected-plan job only when its plan marks that job `not_affected`.
pub(super) fn apply_affected_job_gates(
    jobs: &mut BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    if !jobs.contains_key(PLAN_JOB_ID) {
        return Ok(());
    }
    crate::matrix::crate_job_caps(jobs)?;
    for (id, job) in jobs {
        if !velnor_actions_contract::is_crate_job_id(id) {
            continue;
        }
        if !job.needs.iter().any(|need| need == PLAN_JOB_ID) {
            job.needs.push(PLAN_JOB_ID.to_owned());
        }
        let gate = format!(
            "!contains(needs.{PLAN_JOB_ID}.outputs.{}, ',{id},')",
            crate::matrix::NOT_AFFECTED_JOBS_OUTPUT
        );
        job.condition = Some(match job.condition.take() {
            Some(condition) => format!("({condition}) && {gate}"),
            None => gate,
        });
    }
    Ok(())
}

/// Validate every finalized job after policy merging and internal expansion.
pub(super) fn validate_final_jobs(
    ir: &WorkflowIr,
    jobs: &mut BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    crate::cache_steps::append_bundle_postludes(jobs)?;
    crate::cache_steps::append_workspace_cleanups(jobs)?;
    let mut finalized = ir.clone();
    finalized.jobs.clone_from(jobs);
    finalized.validate().map_err(RenderError::Contract)
}
