// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, steps};
use velnor_actions_contract::workflow::state_seal::STATE_PRODUCER_LANE_CONDITION;
use velnor_actions_contract::{
    Job, RunsOn, ScaleSetSelector, Step, StepKind, StepRole, VerificationRunner,
};

pub(super) fn validate_task_job_scope(
    job_id: &str,
    job: &Job,
    step: &Step,
) -> Result<(), RenderError> {
    if job.check_runner.is_some()
        || !job
            .needs
            .iter()
            .any(|need| need == crate::render::PLAN_JOB_ID)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "declared_task_job_scope_mismatch:{job_id}"
        )));
    }
    if step.id.is_some()
        || step.role.is_some()
        || step
            .condition
            .as_deref()
            .is_some_and(|condition| condition != STATE_PRODUCER_LANE_CONDITION)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "declared_task_authority_mismatch:{job_id}"
        )));
    }
    Ok(())
}

pub(super) fn validate_task_checkout(
    job_id: &str,
    job: &Job,
    step_index: usize,
    checkout_uses: &str,
    report_helper_version: &str,
    controller: &crate::render::ControllerLocator,
) -> Result<(), RenderError> {
    let checkout_at = job.steps[..step_index].iter().position(|previous| {
        velnor_actions_contract::workflow::step_identity::is_configured_checkout(
            previous,
            checkout_uses,
        )
    });
    let Some(checkout_at) = checkout_at else {
        return Err(RenderError::InvalidWorkflow(format!(
            "declared_task_requires_checkout:{job_id}"
        )));
    };
    let staged_binary = format!("{}{report_helper_version}", steps::STAGED_BINARY_PREFIX);
    let helper_staged = job.steps[checkout_at + 1..step_index]
        .iter()
        .any(|previous| {
            if matches!(
                controller,
                crate::render::ControllerLocator::ProtectedBaseDispatch { .. }
            ) {
                steps::protected_preview_acquire_step().is_ok_and(|setup| previous == &setup)
            } else {
                helper_staged_by(previous, &staged_binary)
            }
        });
    if !helper_staged {
        return Err(RenderError::InvalidWorkflow(format!(
            "declared_task_requires_staged_helper:{job_id}"
        )));
    }
    Ok(())
}

pub(super) fn supports_task_runner(
    job_id: &str,
    runs_on: &str,
    workflow_tasks: &[crate::verification_jobs::WorkflowTaskPolicy],
    lane_plan: Option<&velnor_actions_contract::LanePlan>,
    scale_set_selector: Option<&ScaleSetSelector>,
) -> bool {
    match RunsOn::parse(runs_on) {
        Ok(RunsOn::Hosted(label)) => {
            label.starts_with("ubuntu-")
                && velnor_actions_contract::config::RUNNER_LABEL_CATALOG.contains(&label.as_str())
        }
        Ok(RunsOn::ScaleSet(selector)) => {
            let Some(configured) = scale_set_selector else {
                return false;
            };
            if configured != &selector {
                return false;
            }
            workflow_tasks
                .iter()
                .find(|task| task.owns_projected_job_id(job_id, lane_plan))
                .is_none_or(|task| match task {
                    crate::verification_jobs::WorkflowTaskPolicy::Verification(policy) => {
                        policy.task.runner == VerificationRunner::LinuxX64
                            && policy.runner_label == VerificationRunner::LinuxX64.runs_on()
                            && policy.scale_set_token.as_deref() == Some(runs_on)
                    }
                    crate::verification_jobs::WorkflowTaskPolicy::Bun(_)
                    | crate::verification_jobs::WorkflowTaskPolicy::BunSuite(_)
                    | crate::verification_jobs::WorkflowTaskPolicy::BunAssets(_) => true,
                    _ => false,
                })
        }
        Err(_) => false,
    }
}

fn helper_staged_by(step: &Step, staged_binary: &str) -> bool {
    let StepKind::Shell { run, .. } = &step.kind else {
        return false;
    };
    (crate::closure::is_acquire_step(step) || step.role == Some(StepRole::PreseedStage))
        && run.iter().any(|argument| argument.contains(staged_binary))
}
