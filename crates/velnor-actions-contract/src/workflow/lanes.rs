// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::ValidatorKind;
use super::ir::{Job, WorkflowIr};
use super::jobs::{PLAN_JOB_ID, REQUIRED_GATE_JOB_ID, REQUIRED_JOB_ID};
use super::named_check_lanes::add_named_check_lanes;
pub use super::named_check_lanes::{
    NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV, NAMED_CHECK_LANES_ENV, NamedCheckLane,
    NamedCheckLaneVariant, named_check_lanes,
};
use crate::config::{
    CheckExecutor, CheckPlatform, ExecutionConfig, ExecutionMode, ExecutionRole, VelnorConfig,
    VerificationRunner, WORKFLOW_TASK_JOB_PREFIX, WorkflowTask,
};
use crate::errors::ContractError;
use std::collections::BTreeMap;

#[path = "lanes_retarget.rs"]
mod retargeting;
use retargeting::retarget;

pub const HOSTED_SUFFIX: &str = "__hosted";

pub const SCALE_SUFFIX: &str = "__local";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneClass {
    Control,

    SingleWriter,

    Verification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Placement {
    HostedOnly,
    ScaleSetOnly,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmitLane {
    Single,
    Hosted,
    Scale,
}

#[must_use]
pub fn lane_class(job_id: &str) -> LaneClass {
    if job_id == ValidatorKind::Jactionlint.job_id()
        || matches!(
            job_id,
            PLAN_JOB_ID | REQUIRED_JOB_ID | REQUIRED_GATE_JOB_ID | "compare" | "queue-monitor"
        )
    {
        LaneClass::Control
    } else if job_id.starts_with("release-") || job_id.starts_with("deploy-") {
        LaneClass::SingleWriter
    } else {
        LaneClass::Verification
    }
}

pub fn expand_workflow(
    ir: &WorkflowIr,
    config: &VelnorConfig,
    dispatch: Option<ExecutionMode>,
) -> Result<WorkflowIr, ContractError> {
    if config.schema != 2 {
        return Ok(ir.clone());
    }
    let execution = config
        .execution
        .as_ref()
        .ok_or_else(|| ContractError::config("config.toml", "execution", "missing_execution"))?;
    check_override_keys(ir, execution)?;
    let check_lanes = named_check_lanes(ir, config, dispatch)?;
    let map = id_map(ir, config, execution, dispatch)?;
    let mut jobs = std::collections::BTreeMap::new();
    for (id, job) in &ir.jobs {
        let class = lane_class(id);
        let placement = placement_for(config, execution, dispatch, class, id, job)?;
        emit_job(&mut jobs, id, job, execution, class, placement, &map)?;
    }
    let mut expanded = ir.clone();
    expanded.jobs = jobs;
    add_named_check_lanes(&mut expanded, &check_lanes)?;
    Ok(expanded)
}

fn check_override_keys(ir: &WorkflowIr, execution: &ExecutionConfig) -> Result<(), ContractError> {
    for key in execution.overrides.keys() {
        if !ir.jobs.contains_key(key) {
            return Err(ContractError::config(
                "config.toml",
                format!("execution.overrides.{key}"),
                "unknown_logical_job",
            ));
        }
    }
    Ok(())
}

fn id_map(
    ir: &WorkflowIr,
    config: &VelnorConfig,
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
) -> Result<std::collections::BTreeMap<String, Vec<String>>, ContractError> {
    let mut map = std::collections::BTreeMap::new();
    for (id, job) in &ir.jobs {
        let placement = placement_for(config, execution, dispatch, lane_class(id), id, job)?;
        let ids = match placement {
            Placement::Both => vec![
                format!("{id}{HOSTED_SUFFIX}"),
                format!("{id}{SCALE_SUFFIX}"),
            ],
            Placement::HostedOnly | Placement::ScaleSetOnly => vec![id.clone()],
        };
        map.insert(id.clone(), ids);
    }
    Ok(map)
}

pub(super) fn placement_for(
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

fn workflow_task_supports_scale_set(
    job_id: &str,
    job: &Job,
    config: &VelnorConfig,
) -> Result<bool, ContractError> {
    if let Some(check) = &job.check_runner {
        return Ok(
            check.platform == CheckPlatform::LinuxX64 && check.executor == CheckExecutor::Hosted
        );
    }
    if let Some(checked) = config.check_image_job_runner(job_id, &job.runs_on) {
        return checked.map(|()| false);
    }
    let Some(task_id) = job_id.strip_prefix(WORKFLOW_TASK_JOB_PREFIX) else {
        return Ok(true);
    };
    let tasks = &config.workflow.tasks;
    let Some(task) = tasks.iter().find(|task| task.id() == task_id) else {
        return Err(ContractError::config(
            "config.toml",
            format!("workflow.tasks.id:{task_id}"),
            "task_job_without_declaration",
        ));
    };
    let (expected_runner, supports_scale_set) = match task {
        WorkflowTask::Verification(verification) => (
            verification.runner.runs_on(),
            verification.runner == VerificationRunner::LinuxX64,
        ),
        WorkflowTask::Build(build) => (build.runner.runs_on(), false),
        WorkflowTask::NativeImage(image) => (image.platform.runs_on(), false),
    };
    if job.runs_on != expected_runner {
        return Err(ContractError::config(
            "config.toml",
            format!("workflow.tasks.runner:{job_id}"),
            format!("runner_mismatch:{expected_runner}:{}", job.runs_on),
        ));
    }
    Ok(supports_scale_set)
}

fn role_matches(role: ExecutionRole, class: LaneClass) -> bool {
    matches!(
        (role, class),
        (ExecutionRole::Control, LaneClass::Control)
            | (ExecutionRole::Release, LaneClass::SingleWriter)
            | (ExecutionRole::Verification, LaneClass::Verification)
    )
}

fn emit_job(
    jobs: &mut std::collections::BTreeMap<String, Job>,
    id: &str,
    job: &Job,
    execution: &ExecutionConfig,
    class: LaneClass,
    placement: Placement,
    map: &std::collections::BTreeMap<String, Vec<String>>,
) -> Result<(), ContractError> {
    match placement {
        Placement::HostedOnly => {
            insert_job(
                jobs,
                id.to_owned(),
                retarget(job, None, id, id, class, EmitLane::Single, map),
            )?;
        }
        Placement::ScaleSetOnly => {
            let token = execution.scale_selector()?.token();
            let mut next = retarget(job, Some(&token), id, id, class, EmitLane::Single, map);
            next.display_name = scale_name(&job.display_name);
            insert_job(jobs, id.to_owned(), next)?;
        }
        Placement::Both => {
            let token = execution.scale_selector()?.token();
            let hosted_id = format!("{id}{HOSTED_SUFFIX}");
            let scale_id = format!("{id}{SCALE_SUFFIX}");
            let mut hosted = retarget(job, None, id, &hosted_id, class, EmitLane::Hosted, map);
            hosted.display_name = hosted_name(&job.display_name);
            insert_job(jobs, hosted_id, hosted)?;
            let mut scale = retarget(
                job,
                Some(&token),
                id,
                &scale_id,
                class,
                EmitLane::Scale,
                map,
            );
            scale.display_name = scale_name(&job.display_name);
            insert_job(jobs, scale_id, scale)?;
        }
    }
    Ok(())
}

fn insert_job(jobs: &mut BTreeMap<String, Job>, id: String, job: Job) -> Result<(), ContractError> {
    if jobs.contains_key(&id) {
        return Err(ContractError::Collision(format!("expanded job id {id}")));
    }
    jobs.insert(id, job);
    Ok(())
}

fn hosted_name(name: &str) -> String {
    format!("{name} / GitHub hosted / Linux x64")
}

fn scale_name(name: &str) -> String {
    format!("{name} / Velnor Scale Set / Linux x64")
}
