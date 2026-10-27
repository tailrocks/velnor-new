// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::ValidatorKind;
use super::ir::{Job, StepKind, WorkflowIr};
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

#[path = "lanes_bun_plan.rs"]
mod bun_plan;
pub use bun_plan::BunLanePlan;

#[derive(Debug, Clone)]
pub struct WorkflowExpansion {
    pub ir: WorkflowIr,
    pub bun_lane_plan: Option<BunLanePlan>,
}

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

pub fn expand_workflow_with_bun_plan(
    ir: &WorkflowIr,
    config: &VelnorConfig,
    dispatch: Option<ExecutionMode>,
) -> Result<WorkflowExpansion, ContractError> {
    if config.schema != 2 {
        return Ok(WorkflowExpansion {
            ir: ir.clone(),
            bun_lane_plan: None,
        });
    }
    let execution = config
        .execution
        .as_ref()
        .ok_or_else(|| ContractError::config("config.toml", "execution", "missing_execution"))?;
    check_override_keys(ir, execution)?;
    let check_lanes = named_check_lanes(ir, config, dispatch)?;
    let placements = placement_map(ir, config, execution, dispatch)?;
    let map = id_map(&placements);
    let scale_token = if placements
        .values()
        .any(|(_, placement)| matches!(placement, Placement::ScaleSetOnly | Placement::Both))
    {
        Some(execution.scale_selector()?.token())
    } else {
        None
    };
    let _ = bun_asset_producers(&ir.jobs)?;
    let bun_lane_plan = BunLanePlan::from_placements(&placements, &map, scale_token.clone())?;
    let mut jobs = std::collections::BTreeMap::new();
    for (id, job) in &ir.jobs {
        let (class, placement) = placements.get(id).ok_or_else(|| {
            ContractError::identity("workflow.jobs", format!("lane_plan_missing:{id}"))
        })?;
        let emitted = if id.starts_with(crate::config::BUN_JOB_PREFIX) {
            bun_lane_plan.project(id, job)?
        } else {
            emit_job_variants(id, job, scale_token.as_deref(), *class, *placement, &map)?
        };
        for (output_id, output_job) in emitted {
            insert_job(&mut jobs, output_id, output_job)?;
        }
    }
    let mut expanded = ir.clone();
    expanded.jobs = jobs;
    add_named_check_lanes(&mut expanded, &check_lanes)?;
    Ok(WorkflowExpansion {
        ir: expanded,
        bun_lane_plan: Some(bun_lane_plan),
    })
}

fn bun_asset_producers(
    jobs: &BTreeMap<String, Job>,
) -> Result<BTreeMap<String, String>, ContractError> {
    let mut owners = BTreeMap::<String, String>::new();
    for (job_id, job) in jobs {
        for step in &job.steps {
            if let StepKind::Internal { operation, env } = &step.kind
                && operation == super::BUN_ASSETS_EXPORT_OPERATION
                && job_id.starts_with(crate::config::BUN_JOB_PREFIX)
                && let Some(asset_id) = env.get("VELNOR_BUN_ASSET_ID")
                && owners
                    .entry(asset_id.clone())
                    .or_insert_with(|| job_id.clone())
                    .as_str()
                    != job_id.as_str()
            {
                return Err(ContractError::identity(
                    "bun_lane_plan",
                    format!("asset_producer_ambiguous:{asset_id}"),
                ));
            }
        }
    }
    Ok(owners)
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

fn placement_map(
    ir: &WorkflowIr,
    config: &VelnorConfig,
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
) -> Result<BTreeMap<String, (LaneClass, Placement)>, ContractError> {
    let mut map = std::collections::BTreeMap::new();
    for (id, job) in &ir.jobs {
        let class = lane_class(id);
        let placement = placement_for(config, execution, dispatch, class, id, job)?;
        map.insert(id.clone(), (class, placement));
    }
    Ok(map)
}

fn id_map(placements: &BTreeMap<String, (LaneClass, Placement)>) -> BTreeMap<String, Vec<String>> {
    placements
        .iter()
        .map(|(id, (_, placement))| {
            let ids = match placement {
                Placement::Both => vec![
                    format!("{id}{HOSTED_SUFFIX}"),
                    format!("{id}{SCALE_SUFFIX}"),
                ],
                Placement::HostedOnly | Placement::ScaleSetOnly => vec![id.clone()],
            };
            (id.clone(), ids)
        })
        .collect()
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

fn bun_asset_producer(job_id: &str) -> bool {
    job_id.starts_with(crate::config::BUN_JOB_PREFIX) && job_id.ends_with("--assets")
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

fn emit_job_variants(
    id: &str,
    job: &Job,
    scale_token: Option<&str>,
    class: LaneClass,
    placement: Placement,
    map: &std::collections::BTreeMap<String, Vec<String>>,
) -> Result<Vec<(String, Job)>, ContractError> {
    match placement {
        Placement::HostedOnly => Ok(vec![(
            id.to_owned(),
            retarget(job, None, id, id, class, EmitLane::Single, map),
        )]),
        Placement::ScaleSetOnly => {
            let token = scale_token.ok_or_else(missing_scale_selector)?;
            let mut next = retarget(job, Some(token), id, id, class, EmitLane::Single, map);
            next.display_name = scale_name(&job.display_name);
            Ok(vec![(id.to_owned(), next)])
        }
        Placement::Both => {
            let token = scale_token.ok_or_else(missing_scale_selector)?;
            let hosted_id = format!("{id}{HOSTED_SUFFIX}");
            let scale_id = format!("{id}{SCALE_SUFFIX}");
            let mut hosted = retarget(job, None, id, &hosted_id, class, EmitLane::Hosted, map);
            hosted.display_name = hosted_name(&job.display_name);
            let mut scale = retarget(job, Some(token), id, &scale_id, class, EmitLane::Scale, map);
            scale.display_name = scale_name(&job.display_name);
            Ok(vec![(hosted_id, hosted), (scale_id, scale)])
        }
    }
}

fn missing_scale_selector() -> ContractError {
    ContractError::config(
        "config.toml",
        "execution.scale_set_profile",
        "missing_selector",
    )
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
