//! Baseline-publish job: trusted evidence upload after Required passes.
//!
//! Split from `workflow_jobs` so that module keeps the 400-line gate.
//! The job needs the final gate (so it runs only when Required
//! passed), carries a push-plus-protected-ref gate, downloads the
//! plan artifact, stages `baseline.json` through the publish op, and
//! uploads it under the derived artifact name. Publication lives
//! outside merge consumption: the merge never publishes, and this job
//! never judges evidence.

use velnor_actions_contract_workflow::{Job, JobTimeout, Step};
use velnor_actions_workflow_jobs::context::FINAL_JOB_ID;
use velnor_actions_workflow_steps::steps::{PUBLISH_OPERATION, publish_step, write_request_step};

use velnor_actions_orchestrator_core::OrchestratorError;

/// Display name of the baseline-publish job.
pub(crate) const PUBLISH_DISPLAY_NAME: &str = "Publish baseline";

/// Baseline-publish job: plan download, request, publish, upload.
///
/// The job runs only when the final gate passed (`needs` without an
/// `always()` condition) and only for protected-branch pushes (the
/// generated `if:` pins the event plus the generation-time branch).
/// The publish op re-verifies every gate at runtime, so a hand-edited
/// workflow still fails closed. No tool install: the op stages bytes
/// from the downloaded plan with no external commands.
///
/// # Errors
///
/// Returns a contract error for malformed branches or rejected steps.
pub(crate) fn baseline_publish_job(
    label: &str,
    branch: &str,
    acquire: Option<Step>,
) -> Result<Job, OrchestratorError> {
    let mut steps = Vec::new();
    steps.extend(acquire);
    steps.push(crate::matrix_step::download_plan_step()?);
    steps.push(request_step(PUBLISH_OPERATION)?);
    steps.push(publish_step());
    steps.push(
        velnor_actions_workflow_steps::baseline_publish_upload_step().map_err(|err| {
            OrchestratorError::Contract {
                problem: err.to_string(),
            }
        })?,
    );
    Ok(Job {
        check_runner: None,
        display_name: PUBLISH_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        timeout_minutes: JobTimeout::PUBLISH,
        needs: vec![FINAL_JOB_ID.to_owned()],
        condition: Some(publish_gate_condition(branch)?),
        permissions: None,
        environment: None,
        steps,
    })
}

/// Push-plus-protected-ref gate over the generation-time branch.
///
/// # Errors
///
/// Returns a contract error for empty or whitespace-bearing branches.
fn publish_gate_condition(branch: &str) -> Result<String, OrchestratorError> {
    if branch.trim().is_empty() || branch.chars().any(char::is_whitespace) {
        return Err(OrchestratorError::Contract {
            problem: format!("bad_publish_branch:{branch}"),
        });
    }
    Ok(format!(
        "github.event_name == 'push' && github.ref == 'refs/heads/{branch}'"
    ))
}

/// Typed write-request step for the publish target.
fn request_step(target: &str) -> Result<Step, OrchestratorError> {
    write_request_step(target).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })
}

#[cfg(test)]
mod tests;
