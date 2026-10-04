//! Baseline-publish job: trusted evidence upload after Required passes.
//!
//! Split from `workflow_jobs` so that module keeps the 400-line gate.
//! The job needs the final gate (so it runs only when Required
//! passed), carries a push-plus-protected-ref gate, downloads the
//! plan and final-report artifacts, stages `published/baseline.json`, and
//! uploads it under the derived artifact name. Publication lives
//! outside merge consumption: the merge never publishes, and this job
//! never judges evidence.

use velnor_actions_contract::{Job, JobTimeout, Step};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::FINAL_JOB_ID;
use velnor_actions_workflow_renderer::steps::{
    PUBLISH_OPERATION, publish_step, write_request_step,
};

use crate::OrchestratorError;

/// Display name of the baseline-publish job.
pub(crate) const PUBLISH_DISPLAY_NAME: &str = "Publish baseline";

/// Baseline-publish job: exact evidence download, request, publish, upload.
///
/// The job runs only when the final gate passed (`needs` without an
/// `always()` condition) and only for protected-branch pushes (the
/// generated `if:` pins the event plus the generation-time branch).
/// The publish op re-verifies every gate at runtime, so a hand-edited
/// workflow still fails closed. Pinned `gh` retrieves prior baseline
/// evidence when the plan carries covered obligations.
///
/// # Errors
///
/// Returns a contract error for malformed branches or rejected steps.
pub(crate) fn baseline_publish_job(
    label: &str,
    branch: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
) -> Result<Job, OrchestratorError> {
    let mut steps = Vec::new();
    steps.extend(acquire);
    steps.push(crate::matrix_step::download_plan_step()?);
    steps.push(download_final_report_step()?);
    steps.push(crate::workflow_jobs::prepare_pinned_tools_step_for_runner(
        catalog,
        vec![PinnedTool::Gh],
        true,
        label,
    )?);
    steps.push(request_step(PUBLISH_OPERATION)?);
    steps.push(publish_step());
    steps.push(
        velnor_actions_workflow_renderer::baseline_publish_upload_step().map_err(|err| {
            OrchestratorError::Contract {
                problem: err.to_string(),
            }
        })?,
    );
    Ok(Job {
        cache_mode: None,
        display_name: PUBLISH_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        timeout_minutes: JobTimeout::PUBLISH,
        needs: vec![FINAL_JOB_ID.to_owned()],
        condition: Some(publish_gate_condition(branch)?),
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        steps,
    })
}

/// Current-run final evidence: exact name and attempt, no artifact wildcard.
fn download_final_report_step() -> Result<Step, OrchestratorError> {
    let name = format!(
        "velnor-final-{}",
        velnor_actions_workflow_renderer::steps::RUN_KEY_EXPR
    );
    let mut step = velnor_actions_workflow_renderer::steps::download_artifact_step(
        &name,
        velnor_actions_workflow_renderer::closure::PLAN_ARTIFACT_PATH,
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    "Download final report".clone_into(&mut step.name);
    Ok(step)
}

/// Push-plus-protected-ref gate over the generation-time branch.
///
/// # Errors
///
/// Returns a contract error for unsupported literal branch names.
fn publish_gate_condition(branch: &str) -> Result<String, OrchestratorError> {
    if !velnor_actions_contract::is_valid_branch_name(branch) {
        return Err(OrchestratorError::Contract {
            problem: format!("bad_publish_branch:{branch}"),
        });
    }
    Ok(format!(
        "github.event_name == 'push' && github.ref == 'refs/heads/{branch}' && github.ref_protected == true"
    ))
}

/// Typed write-request step for the publish target.
fn request_step(target: &str) -> Result<Step, OrchestratorError> {
    write_request_step(target).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })
}

#[cfg(test)]
#[path = "publish_job_tests.rs"]
mod publish_job_tests;

#[cfg(test)]
mod branch_tests {
    use super::publish_gate_condition;

    #[test]
    fn publication_gate_rejects_branch_expression_injection() {
        for branch in ["main'||true||'", "a|b", "a&b", "a.lock", "-a", "a\nb"] {
            assert!(publish_gate_condition(branch).is_err(), "{branch:?}");
        }
        assert_eq!(
            publish_gate_condition("release/1.2").expect("literal branch"),
            "github.event_name == 'push' && github.ref == 'refs/heads/release/1.2' && github.ref_protected == true"
        );
    }
}
