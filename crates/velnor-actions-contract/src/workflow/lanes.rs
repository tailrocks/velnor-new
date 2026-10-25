//! Expand one logical job graph into hosted and scale-set lanes.
//!
//! Schema 1 returns the same jobs. Control and single-writer jobs stay one hosted job.
//! `hosted` keeps configured runners. `scale-set` routes every supported job once.
//! `both` duplicates eligible verification only.

use super::ValidatorKind;
use super::ir::WorkflowIr;
use super::jobs::{PLAN_JOB_ID, REQUIRED_GATE_JOB_ID, REQUIRED_JOB_ID};
use super::named_check_lanes::add_named_check_lanes;
pub use super::named_check_lanes::{
    NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV, NAMED_CHECK_LANES_ENV, NamedCheckLane,
    NamedCheckLaneVariant, named_check_lanes, qualification_job_lanes,
};
#[path = "lanes_hosted_tasks.rs"]
mod hosted_tasks;
#[path = "lanes_placement.rs"]
mod placement;
use crate::config::{ExecutionConfig, ExecutionMode, VelnorConfig};
use crate::errors::ContractError;
use placement::placement_for_executor;
pub(super) use placement::{placement_for, placement_for_source};
use std::collections::BTreeMap;
mod emit;
#[path = "lanes_retarget.rs"]
mod retargeting;
use emit::emit_job;

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

/// Where one logical job is emitted.
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
    expand_workflow_with_qualification_sources(ir, config, dispatch, &BTreeMap::new())
}

/// Expand jobs using source identities for selected crate jobs.
///
/// The source map keeps runner overrides when a selected job uses another ID.
/// # Errors
pub fn expand_workflow_with_qualification_sources(
    ir: &WorkflowIr,
    config: &VelnorConfig,
    dispatch: Option<ExecutionMode>,
    qualification_sources: &std::collections::BTreeMap<String, String>,
) -> Result<WorkflowIr, ContractError> {
    if config.schema != 2 {
        return Ok(ir.clone());
    }
    let execution = config
        .execution
        .as_ref()
        .ok_or_else(|| ContractError::config("config.toml", "execution", "missing_execution"))?;
    check_override_keys(ir, execution)?;
    let mut executor_lanes = named_check_lanes(ir, config, dispatch)?;
    for (job_id, lanes) in qualification_job_lanes(ir, config, dispatch, qualification_sources)? {
        if executor_lanes.insert(job_id, lanes).is_some() {
            return Err(ContractError::identity(
                "workflow.executor_lanes",
                "duplicate_executor_lane_map",
            ));
        }
    }
    let map = id_map(ir, config, execution, dispatch, qualification_sources)?;
    let mut jobs = std::collections::BTreeMap::new();
    for (id, job) in &ir.jobs {
        let class = lane_class(id);
        let placement = placement_for_executor(
            config,
            execution,
            dispatch,
            class,
            id,
            job,
            qualification_sources,
        )?;
        emit_job(&mut jobs, id, job, execution, class, placement, &map)?;
    }
    let mut expanded = ir.clone();
    expanded.jobs = jobs;
    add_named_check_lanes(&mut expanded, &executor_lanes)?;
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
    qualification_sources: &std::collections::BTreeMap<String, String>,
) -> Result<std::collections::BTreeMap<String, Vec<String>>, ContractError> {
    let mut map = std::collections::BTreeMap::new();
    for (id, job) in &ir.jobs {
        let placement = placement_for_executor(
            config,
            execution,
            dispatch,
            lane_class(id),
            id,
            job,
            qualification_sources,
        )?;
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
