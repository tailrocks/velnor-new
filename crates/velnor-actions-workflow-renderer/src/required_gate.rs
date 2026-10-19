//! The branch-protection `Required` gate over the verdict job.
//!
//! GitHub reports a skipped job's check as successful, and branch
//! protection accepts it. The verdict job (`required`, `merge-v1`) runs
//! under `!cancelled()` so a cancelled run never spends minutes judging
//! reports while holding the concurrency group; a cancelled run therefore
//! skips it. The protected check must come from a job that never ends
//! skipped: the gate runs under `always()`, needs only the verdict, and
//! its single runner-only step fails unless the verdict succeeded. A
//! cancelled run's gate fails within one runner start, and a superseding
//! run (same concurrency group) reports its own gate afterwards.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    Job, JobTimeout, REQUIRED_DISPLAY_NAME, REQUIRED_GATE_CONDITION, REQUIRED_GATE_FAIL_CONDITION,
    REQUIRED_GATE_JOB_ID,
};

use crate::RenderError;
use crate::render::FINAL_JOB_ID;

/// Gate step display name.
const GATE_STEP_NAME: &str = "Fail unless the verdict passed";

/// Insert the gate when the workflow has a verdict job.
///
/// # Errors
///
/// Returns a policy error when a job already uses the gate ID.
pub(crate) fn insert_required_gate(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    let Some(verdict) = jobs.get(FINAL_JOB_ID) else {
        return Ok(());
    };
    let mut step =
        crate::steps::shell_step(GATE_STEP_NAME, vec!["false".to_owned()], BTreeMap::new())?;
    step.condition = Some(REQUIRED_GATE_FAIL_CONDITION.to_owned());
    let gate = Job {
        display_name: REQUIRED_DISPLAY_NAME.to_owned(),
        runs_on: verdict.runs_on.clone(),
        check_runner: None,
        timeout_minutes: JobTimeout::REQUIRED,
        needs: vec![FINAL_JOB_ID.to_owned()],
        condition: Some(REQUIRED_GATE_CONDITION.to_owned()),
        permissions: None,
        cache_mode: None,
        environment: None,
        steps: vec![step],
    };
    crate::support::insert_support_job(jobs, REQUIRED_GATE_JOB_ID, gate, "required-gate")
}

/// The gate must exist exactly over the verdict, unchanged.
///
/// # Errors
///
/// Returns an invalid-workflow error for a missing or altered gate.
pub(crate) fn check_required_gate(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    if !jobs.contains_key(FINAL_JOB_ID) {
        return Ok(());
    }
    let bad = |problem: &str| Err(RenderError::InvalidWorkflow(problem.to_owned()));
    let Some(gate) = jobs.get(REQUIRED_GATE_JOB_ID) else {
        return bad("missing_required_gate");
    };
    if gate.display_name != REQUIRED_DISPLAY_NAME
        || gate.condition.as_deref() != Some(REQUIRED_GATE_CONDITION)
        || gate.needs != [FINAL_JOB_ID]
        || gate.steps.len() != 1
        || gate
            .steps
            .iter()
            .any(|step| step.condition.as_deref() != Some(REQUIRED_GATE_FAIL_CONDITION))
    {
        return bad("bad_required_gate");
    }
    if jobs
        .iter()
        .any(|(id, job)| id != REQUIRED_GATE_JOB_ID && job.display_name == REQUIRED_DISPLAY_NAME)
    {
        return bad("duplicate_required_check_name");
    }
    Ok(())
}

#[cfg(test)]
#[path = "required_gate_tests.rs"]
mod tests;
