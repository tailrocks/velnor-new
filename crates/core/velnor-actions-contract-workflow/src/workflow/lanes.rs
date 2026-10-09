//! Expand one logical job graph into hosted and scale-set lanes.
//!
//! Schema 1 returns the same jobs. Control and single-writer jobs stay
//! one hosted job. `both` duplicates eligible verification only.

use super::ir::{Job, WorkflowIr};
use super::jobs::{PLAN_JOB_ID, REQUIRED_JOB_ID};
use super::named_check_lanes::add_named_check_lanes;
pub use super::named_check_lanes::{
    EXECUTION_MODE_ENV, NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV,
    NAMED_CHECK_LANES_ENV, NamedCheckLane, NamedCheckLaneVariant, named_check_lanes,
};
use std::collections::BTreeMap;
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract_config::config::{
    CheckExecutor, CheckPlatform, EPHEMERAL_CHECK_ADMISSION_CONDITION, ExecutionConfig,
    ExecutionMode, ExecutionRole, VERIFICATION_TASK_JOB_PREFIX, ValidatorKind, VelnorConfig,
    VerificationRunner,
};

mod verification_mode;
pub use verification_mode::verification_job_mode;

/// Suffix for the hosted copy of a verification job.
pub const HOSTED_SUFFIX: &str = "__hosted";
/// Suffix for the scale-set copy of a verification job.
pub const SCALE_SUFFIX: &str = "__local";
/// Planner class of a job id. Role overrides cannot change this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneClass {
    /// Trusted control job.
    Control,
    /// Single external writer.
    SingleWriter,
    /// Comparable verification workload.
    Verification,
}

/// Where one verification workload is emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Placement {
    HostedOnly,
    ScaleSetOnly,
    Both,
}

/// Which copy of a split dependency a consumer keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmitLane {
    Single,
    Hosted,
    Scale,
}

/// Classify a planner job id.
#[must_use]
pub fn lane_class(job_id: &str) -> LaneClass {
    if job_id == ValidatorKind::Actionlint.job_id()
        || matches!(
            job_id,
            PLAN_JOB_ID | REQUIRED_JOB_ID | "compare" | "queue-monitor"
        )
    {
        LaneClass::Control
    } else if job_id == "publish-baseline"
        || job_id.starts_with("release-")
        || job_id.starts_with("deploy-")
    {
        LaneClass::SingleWriter
    } else {
        LaneClass::Verification
    }
}

/// Expand jobs for schema 2. Schema 1 is returned unchanged.
///
/// # Errors
///
/// Unknown profiles, unknown override keys, or a role that disagrees
/// with [`lane_class`].
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
    let effective_mode = effective_execution_mode(execution, dispatch);
    let check_lanes = named_check_lanes(ir, config, dispatch)?;
    let map = id_map(ir, execution, dispatch)?;
    let mut jobs = std::collections::BTreeMap::new();
    for (id, job) in &ir.jobs {
        let class = lane_class(id);
        let placement = placement_for(execution, dispatch, class, id, job)?;
        emit_job(&mut jobs, id, job, execution, class, placement, &map)?;
    }
    let mut expanded = ir.clone();
    expanded.jobs = jobs;
    add_named_check_lanes(&mut expanded, &check_lanes, effective_mode)?;
    Ok(expanded)
}

fn effective_execution_mode(
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
) -> ExecutionMode {
    dispatch.or(execution.mode).unwrap_or_else(|| {
        if execution.default_profile == execution.scale_set_profile {
            ExecutionMode::ScaleSet
        } else {
            ExecutionMode::Hosted
        }
    })
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
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
) -> Result<std::collections::BTreeMap<String, Vec<String>>, ContractError> {
    let mut map = std::collections::BTreeMap::new();
    for (id, job) in &ir.jobs {
        let placement = placement_for(execution, dispatch, lane_class(id), id, job)?;
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
    if !verification_task_supports_scale_set(job_id, job)? {
        if execution
            .overrides
            .get(job_id)
            .is_some_and(|over| over.profile == execution.scale_set_profile)
        {
            return Err(ContractError::config(
                "config.toml",
                format!("execution.overrides.{job_id}.profile"),
                "verification_runner_incompatible_with_scale_set",
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

/// Native macOS tasks cannot use the Linux/amd64 Scale Set profile.
fn verification_task_supports_scale_set(job_id: &str, job: &Job) -> Result<bool, ContractError> {
    if let Some(check) = &job.check_runner {
        return Ok(
            check.platform == CheckPlatform::LinuxX64 && check.executor == CheckExecutor::Hosted
        );
    }
    if !job_id.starts_with(VERIFICATION_TASK_JOB_PREFIX) {
        return Ok(true);
    }
    match job.runs_on.as_str() {
        label if label == VerificationRunner::LinuxX64.runs_on() => Ok(true),
        label if label == VerificationRunner::MacosArm64.runs_on() => Ok(false),
        label => Err(ContractError::config(
            "config.toml",
            format!("workflow.tasks.runner:{job_id}"),
            format!("unsupported_verification_runner:{label}"),
        )),
    }
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

fn retarget(
    job: &Job,
    runs_on: Option<&str>,
    source_id: &str,
    output_id: &str,
    class: LaneClass,
    lane: EmitLane,
    map: &std::collections::BTreeMap<String, Vec<String>>,
) -> Job {
    let mut next = job.clone();
    if let Some(token) = runs_on {
        token.clone_into(&mut next.runs_on);
        if next.check_runner.is_some() {
            next.condition = Some(EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned());
        }
    }
    retarget_check_identity(&mut next, source_id, output_id, lane);
    next.needs = expand_needs(&job.needs, map, class, lane);
    next
}

fn retarget_check_identity(job: &mut Job, source_id: &str, output_id: &str, lane: EmitLane) {
    for step in &mut job.steps {
        match &mut step.kind {
            super::step::StepKind::Shell { env, .. } => {
                if let Some(job_id) = env.get_mut(NAMED_CHECK_JOB_ID_ENV)
                    && job_id == source_id
                {
                    output_id.clone_into(job_id);
                }
                if let Some(variant) = env.get_mut(NAMED_CHECK_LANE_VARIANT_ENV) {
                    match lane {
                        EmitLane::Hosted => "hosted".clone_into(variant),
                        EmitLane::Scale => "scale_set".clone_into(variant),
                        EmitLane::Single => {}
                    }
                }
            }
            super::step::StepKind::Action { with, .. } => {
                if let Some(name) = with.get_mut("name")
                    && name.starts_with("velnor-crate-")
                    && name.ends_with(&format!("-{source_id}"))
                {
                    let prefix_len = name.len() - source_id.len();
                    name.truncate(prefix_len);
                    name.push_str(output_id);
                }
            }
            super::step::StepKind::Internal { .. } => {}
        }
    }
}

fn expand_needs(
    needs: &[String],
    map: &std::collections::BTreeMap<String, Vec<String>>,
    class: LaneClass,
    lane: EmitLane,
) -> Vec<String> {
    let mut out = Vec::new();
    for need in needs {
        let targets = map.get(need).cloned().unwrap_or_else(|| vec![need.clone()]);
        if class != LaneClass::Verification || targets.len() == 1 {
            out.extend(targets);
            continue;
        }
        let suffix = match lane {
            EmitLane::Hosted => HOSTED_SUFFIX,
            EmitLane::Scale => SCALE_SUFFIX,
            EmitLane::Single => "",
        };
        if suffix.is_empty() {
            out.extend(targets);
        } else {
            out.extend(targets.into_iter().filter(|id| id.ends_with(suffix)));
        }
    }
    out
}

fn hosted_name(name: &str) -> String {
    format!("{name} / GitHub hosted / Linux x64")
}

fn scale_name(name: &str) -> String {
    format!("{name} / Velnor Scale Set / Linux x64")
}
