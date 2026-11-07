// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    ContractError, ExecutionConfig, ExecutionMode, Job, LaneClass, Placement, VelnorConfig,
    WORKFLOW_TASK_JOB_PREFIX, WorkflowTask, bun_asset_producer, role_matches,
    workflow_task_supports_scale_set,
};

pub(in crate::workflow) fn placement_for(
    config: &VelnorConfig,
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
    class: LaneClass,
    job_id: &str,
    job: &Job,
) -> Result<Placement, ContractError> {
    if let Some(over) = execution.overrides.get(job_id)
        && !role_matches(over.role, class)
    {
        return Err(ContractError::config(
            "config.toml",
            format!("execution.overrides.{job_id}.role"),
            "role_not_eligible",
        ));
    }
    if class != LaneClass::Verification {
        return Ok(Placement::HostedOnly);
    }
    if bun_asset_producer(job_id) {
        if execution
            .overrides
            .get(job_id)
            .is_some_and(|over| over.profile != execution.hosted_profile)
        {
            return Err(ContractError::config(
                "config.toml",
                format!("execution.overrides.{job_id}.profile"),
                "bun_private_assets_hosted_only",
            ));
        }
        return Ok(Placement::HostedOnly);
    }
    if !workflow_task_supports_scale_set(job_id, job, config)? {
        if execution
            .overrides
            .get(job_id)
            .is_some_and(|over| over.profile == execution.scale_set_profile)
        {
            let hosted_only = config.image_for_build_job(job_id).is_some()
                || config.workflow.tasks.iter().any(|task| {
                    matches!(task, WorkflowTask::Build(_) | WorkflowTask::NativeImage(_))
                        && format!("{WORKFLOW_TASK_JOB_PREFIX}{}", task.id()) == job_id
                });
            let issue = if hosted_only {
                "workflow_task_hosted_only_scale_set_override"
            } else {
                "verification_runner_incompatible_with_scale_set"
            };
            return Err(ContractError::config(
                "config.toml",
                format!("execution.overrides.{job_id}.profile"),
                issue,
            ));
        }
        return Ok(Placement::HostedOnly);
    }
    if let Some(mode) = dispatch.or(execution.mode) {
        return Ok(match mode {
            ExecutionMode::Hosted => Placement::HostedOnly,
            ExecutionMode::ScaleSet => Placement::ScaleSetOnly,
            ExecutionMode::Both if job_id.starts_with(crate::config::BUN_JOB_PREFIX) => {
                Placement::HostedOnly
            }
            ExecutionMode::Both => Placement::Both,
        });
    }
    let key = execution
        .overrides
        .get(job_id)
        .map_or(execution.default_profile.as_str(), |over| {
            over.profile.as_str()
        });
    if key == execution.hosted_profile {
        Ok(Placement::HostedOnly)
    } else if key == execution.scale_set_profile {
        Ok(Placement::ScaleSetOnly)
    } else {
        Err(ContractError::config(
            "config.toml",
            format!("execution.overrides.{job_id}.profile"),
            format!("unknown_profile:{key}"),
        ))
    }
}
