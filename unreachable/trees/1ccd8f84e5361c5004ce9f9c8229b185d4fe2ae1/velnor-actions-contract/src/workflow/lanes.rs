//! Expand one logical job graph into hosted and scale-set lanes.
//!
//! Schema 1 returns the same jobs. Control and single-writer jobs stay
//! one hosted job. `both` duplicates eligible verification only.

use crate::config::{ExecutionConfig, ExecutionMode, ExecutionRole, VelnorConfig};
use crate::errors::ContractError;

use super::ir::{Job, WorkflowIr};
use super::jobs::{PLAN_JOB_ID, REQUIRED_JOB_ID};

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
enum Placement {
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
    if matches!(
        job_id,
        PLAN_JOB_ID | REQUIRED_JOB_ID | "compare" | "queue-monitor"
    ) {
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
    let map = id_map(ir, execution, dispatch)?;
    let mut jobs = std::collections::BTreeMap::new();
    for (id, job) in &ir.jobs {
        let class = lane_class(id);
        let placement = placement_for(execution, dispatch, class, id)?;
        emit_job(&mut jobs, id, job, execution, class, placement, &map)?;
    }
    let mut expanded = ir.clone();
    expanded.jobs = jobs;
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
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
) -> Result<std::collections::BTreeMap<String, Vec<String>>, ContractError> {
    let mut map = std::collections::BTreeMap::new();
    for id in ir.jobs.keys() {
        let placement = placement_for(execution, dispatch, lane_class(id), id)?;
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

fn placement_for(
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
    class: LaneClass,
    job_id: &str,
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
            jobs.insert(
                id.to_owned(),
                retarget(job, None, class, EmitLane::Single, map),
            );
        }
        Placement::ScaleSetOnly => {
            let token = execution.scale_selector()?.token();
            let mut next = retarget(job, Some(&token), class, EmitLane::Single, map);
            next.display_name = scale_name(&job.display_name);
            jobs.insert(id.to_owned(), next);
        }
        Placement::Both => {
            let token = execution.scale_selector()?.token();
            let mut hosted = retarget(job, None, class, EmitLane::Hosted, map);
            hosted.display_name = hosted_name(&job.display_name);
            jobs.insert(format!("{id}{HOSTED_SUFFIX}"), hosted);
            let mut scale = retarget(job, Some(&token), class, EmitLane::Scale, map);
            scale.display_name = scale_name(&job.display_name);
            jobs.insert(format!("{id}{SCALE_SUFFIX}"), scale);
        }
    }
    Ok(())
}

fn retarget(
    job: &Job,
    runs_on: Option<&str>,
    class: LaneClass,
    lane: EmitLane,
    map: &std::collections::BTreeMap<String, Vec<String>>,
) -> Job {
    let mut next = job.clone();
    if let Some(token) = runs_on {
        token.clone_into(&mut next.runs_on);
    }
    next.needs = expand_needs(&job.needs, map, class, lane);
    next
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
