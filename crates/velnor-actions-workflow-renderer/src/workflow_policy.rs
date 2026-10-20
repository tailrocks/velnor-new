//! Exact top-level workflow policy checks shared by render entrypoints.

use velnor_actions_contract::{Concurrency, Trigger, WorkflowIr};

use crate::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, RenderError,
    verification_jobs::WorkflowTaskPolicy,
};

/// Require the exact trigger shape: 4 PR types, one push branch, merge group.
pub(crate) fn check_triggers(triggers: &Trigger) -> Result<(), RenderError> {
    let expected: Vec<String> = EXPECTED_PR_TYPES.iter().map(ToString::to_string).collect();
    if triggers.pull_request_types != expected {
        return Err(RenderError::InvalidWorkflow("bad_pr_triggers".to_owned()));
    }
    let branch_ok = triggers.push_branches.len() == 1
        && triggers
            .push_branches
            .first()
            .is_some_and(|branch| velnor_actions_contract::is_valid_branch_name(branch));
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
    workflow_tasks: &[WorkflowTaskPolicy],
) -> Result<(), RenderError> {
    for (id, job) in &ir.jobs {
        if let Some(runner) = &job.check_runner {
            if !check_runner_is_valid(id, job, runner) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "check_runner_mismatch:{id}"
                )));
            }
        } else {
            let task_label = workflow_tasks
                .iter()
                .find(|task| task.owns_job_id(id))
                .and_then(|task| task.runner_label_for_job(id));
            let suite_runner_ok = id.starts_with("suite-")
                && velnor_actions_contract::RunsOn::parse(&job.runs_on).is_ok();
            if job.runs_on != label
                && task_label != Some(job.runs_on.as_str())
                && !suite_runner_ok
                && !velnor_actions_contract::RunsOn::parse(&job.runs_on)
                    .is_ok_and(|selector| selector.is_scale_set())
            {
                return Err(RenderError::InvalidWorkflow(format!("label_mismatch:{id}")));
            }
        }
    }
    Ok(())
}

fn check_runner_is_valid(
    id: &str,
    job: &velnor_actions_contract::Job,
    runner: &velnor_actions_contract::CheckRunner,
) -> bool {
    if !id.starts_with("check-") {
        return false;
    }
    let scale_set = velnor_actions_contract::RunsOn::parse(&job.runs_on)
        .is_ok_and(|selector| selector.is_scale_set());
    if !scale_set {
        return runner.label == job.runs_on;
    }

    runner.platform == velnor_actions_contract::CheckPlatform::LinuxX64
        && runner.executor == velnor_actions_contract::CheckExecutor::Hosted
        && scale_set_check_admission(job.condition.as_deref())
}

fn scale_set_check_admission(condition: Option<&str>) -> bool {
    condition.is_some_and(|condition| {
        condition == velnor_actions_contract::config::EPHEMERAL_CHECK_ADMISSION_CONDITION
            || condition
                == "always() && needs.plan.result == 'success' && ((needs.plan.outputs.qualification_campaign == '' && (github.event_name == 'push' || github.event_name == 'merge_group' || (github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository))) || needs.plan.outputs.qualification_campaign != '')"
    })
}

#[cfg(test)]
#[path = "workflow_policy_tests.rs"]
mod tests;
