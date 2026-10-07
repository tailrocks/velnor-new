//! Exact top-level workflow policy checks shared by render entrypoints.

use velnor_actions_contract_workflow::{Concurrency, Trigger, WorkflowIr};

use crate::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, VerificationTaskPolicy};
use velnor_actions_workflow_steps::RenderError;

/// Require the exact trigger shape: 4 PR types, one push branch, merge group.
pub(crate) fn check_triggers(triggers: &Trigger) -> Result<(), RenderError> {
    let expected: Vec<String> = EXPECTED_PR_TYPES.iter().map(ToString::to_string).collect();
    if triggers.pull_request_types != expected {
        return Err(RenderError::InvalidWorkflow("bad_pr_triggers".to_owned()));
    }
    let branch_ok = triggers.push_branches.len() == 1
        && triggers.push_branches.first().is_some_and(|branch| {
            !branch.trim().is_empty() && !branch.chars().any(char::is_whitespace)
        });
    if !branch_ok {
        return Err(RenderError::InvalidWorkflow("bad_push_branch".to_owned()));
    }
    if !triggers.merge_group {
        return Err(RenderError::InvalidWorkflow(
            "missing_merge_group".to_owned(),
        ));
    }
    Ok(())
}

/// Require the exact concurrency group plus PR-only cancel.
pub(crate) fn check_concurrency(concurrency: &Concurrency) -> Result<(), RenderError> {
    if concurrency.group != CONCURRENCY_GROUP
        || concurrency.cancel_in_progress != CONCURRENCY_CANCEL
    {
        return Err(RenderError::InvalidWorkflow("bad_concurrency".to_owned()));
    }
    Ok(())
}

/// Require every job to use the single context label.
pub(crate) fn check_single_label(
    ir: &WorkflowIr,
    label: &str,
    verification_tasks: &[VerificationTaskPolicy],
) -> Result<(), RenderError> {
    for (id, job) in &ir.jobs {
        if let Some(runner) = &job.check_runner {
            let scale_set = velnor_actions_contract_config::RunsOn::parse(&job.runs_on)
                .is_ok_and(|selector| selector.is_scale_set());
            let valid_placement = if scale_set {
                runner.platform == velnor_actions_contract_config::CheckPlatform::LinuxX64
                    && runner.executor == velnor_actions_contract_config::CheckExecutor::Hosted
                    && job.condition.as_deref()
                        == Some(
                            velnor_actions_contract_config::config::EPHEMERAL_CHECK_ADMISSION_CONDITION,
                        )
            } else {
                runner.label == job.runs_on
            };
            if !id.starts_with("check-") || !valid_placement {
                return Err(RenderError::InvalidWorkflow(format!(
                    "check_runner_mismatch:{id}"
                )));
            }
        } else {
            let task_label = verification_tasks
                .iter()
                .find(|task| task.owns_job_id(id))
                .map(|task| task.runner_label.as_str());
            if job.runs_on != label
                && task_label != Some(job.runs_on.as_str())
                && !velnor_actions_contract_config::RunsOn::parse(&job.runs_on)
                    .is_ok_and(|selector| selector.is_scale_set())
            {
                return Err(RenderError::InvalidWorkflow(format!("label_mismatch:{id}")));
            }
        }
    }
    Ok(())
}
