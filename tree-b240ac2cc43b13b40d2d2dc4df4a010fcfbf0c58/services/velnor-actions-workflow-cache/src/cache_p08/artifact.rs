//! Validate the deliberate cache-free artifact build setup.

use velnor_actions_contract_workflow::{
    ARTIFACT_MATRIX_MAX_PARALLEL_ENV, ARTIFACT_MATRIX_NEEDS_JOB_ENV, ARTIFACT_MATRIX_PROVIDER_ENV,
    Job, StepKind, StepRole,
};
use velnor_actions_workflow_steps::{MiseSetup, RenderError};

use super::{check_setup_before_mise, is_setup_step, shape::setup_shape_ok};

/// Validate the pinned, cache-disabled Mise setup for a dynamic artifact task.
///
/// Artifact task commands are selected from the plan matrix at runtime, so a
/// static tool union cannot be derived from their argv. These jobs deliberately
/// remain cold: they use the pinned Mise binary and `mise install --locked`,
/// without reading or writing the tools cache.
/// # Errors
pub fn require_uncached_setup(
    job_id: &str,
    job: &Job,
    setup: &MiseSetup,
    checkout_uses: &str,
) -> Result<(), RenderError> {
    setup.validate()?;
    if !has_artifact_matrix_markers(job) {
        return Err(RenderError::InvalidWorkflow(format!(
            "uncached_setup_without_artifact_matrix:{job_id}"
        )));
    }
    let present: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_setup_step(step))
        .map(|(index, _)| index)
        .collect();
    if present.len() != 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "uncached_setup_count:{job_id}"
        )));
    }
    let index = present[0];
    let step = &job.steps[index];
    if step.role.is_some_and(|role| role != StepRole::MiseSetup)
        || !setup_shape_ok(step, setup, false, None)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "uncached_setup_malformed:{job_id}"
        )));
    }
    if !job.steps[..index].iter().any(|candidate| {
        velnor_actions_contract_workflow::workflow::step_identity::is_configured_checkout(
            candidate,
            checkout_uses,
        )
    }) {
        return Err(RenderError::InvalidWorkflow(format!(
            "uncached_setup_without_checkout:{job_id}"
        )));
    }
    crate::tool_seed::reject_orphan_seed(job_id, job)?;
    check_setup_before_mise(job_id, job, index)
}

/// True when the job is selected by the complete renderer-owned artifact matrix marker set.
#[must_use]
pub fn has_artifact_matrix_markers(job: &Job) -> bool {
    job.steps.iter().any(|step| {
        let StepKind::Shell { env, .. } = &step.kind else {
            return false;
        };
        env.contains_key(ARTIFACT_MATRIX_NEEDS_JOB_ENV)
            && env.contains_key(ARTIFACT_MATRIX_PROVIDER_ENV)
            && env.contains_key(ARTIFACT_MATRIX_MAX_PARALLEL_ENV)
    })
}
