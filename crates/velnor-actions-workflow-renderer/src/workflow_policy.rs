// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{BunLanePlan, Concurrency, Trigger, WorkflowIr};

use crate::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, RenderError,
    verification_jobs::WorkflowTaskPolicy,
};

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

pub(crate) fn check_concurrency(concurrency: &Concurrency) -> Result<(), RenderError> {
    if concurrency.group != CONCURRENCY_GROUP
        || concurrency.cancel_in_progress != CONCURRENCY_CANCEL
    {
        return Err(RenderError::InvalidWorkflow("bad_concurrency".to_owned()));
    }
    Ok(())
}

pub(crate) fn check_single_label(
    ir: &WorkflowIr,
    label: &str,
    workflow_tasks: &[WorkflowTaskPolicy],
    bun_lane_plan: Option<&BunLanePlan>,
) -> Result<(), RenderError> {
    for (id, job) in &ir.jobs {
        if let Some(runner) = &job.check_runner {
            let scale_set = velnor_actions_contract::RunsOn::parse(&job.runs_on)
                .is_ok_and(|selector| selector.is_scale_set());
            let valid_placement = if scale_set {
                runner.platform == velnor_actions_contract::CheckPlatform::LinuxX64
                    && runner.executor == velnor_actions_contract::CheckExecutor::Hosted
                    && job.condition.as_deref()
                        == Some(
                            velnor_actions_contract::config::EPHEMERAL_CHECK_ADMISSION_CONDITION,
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
            let task_label = workflow_tasks
                .iter()
                .find(|task| task.owns_projected_job_id(id, bun_lane_plan))
                .and_then(|task| task.runner_label_for_projected_job(id, bun_lane_plan));
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
