//! Exact named-check job/report identities shared by workflow and plan.

use super::ir::WorkflowIr;
use super::jobs::PLAN_JOB_ID;
use super::lanes::{Placement, placement_for, placement_for_source};
use super::step::StepKind;
use super::{HOSTED_SUFFIX, SCALE_SUFFIX, lane_class};
use crate::config::{ExecutionMode, VelnorConfig};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Env key carrying the static named-check output identities into `plan-v1`.
pub const NAMED_CHECK_LANES_ENV: &str = "VELNOR_NAMED_CHECK_LANES_JSON";
/// Prefix for separate per-entry Qualification jobs for named checks.
pub const QUALIFICATION_CHECK_JOB_PREFIX: &str = "qualification-check-";

pub(super) fn qualification_check_source_id(job_id: &str) -> Option<String> {
    job_id
        .strip_prefix(QUALIFICATION_CHECK_JOB_PREFIX)
        .map(|check_id| format!("check-{check_id}"))
}
/// Env key binding a check worker to its exact report-owning job ID.
pub const NAMED_CHECK_JOB_ID_ENV: &str = "VELNOR_CHECK_JOB_ID";
/// Env key binding a check worker to its exact lane variant.
pub const NAMED_CHECK_LANE_VARIANT_ENV: &str = "VELNOR_CHECK_LANE_VARIANT";

/// One concrete job/report lane for a named check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedCheckLane {
    /// Set only for duplicated hosted/Scale Set pairs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<NamedCheckLaneVariant>,
    /// Exact emitted job ID; binds the plan entry to its upload artifact.
    pub job_id: String,
    /// Exact runner selector emitted for this lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs_on: Option<String>,
}

/// Typed variant of a duplicated named-check lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedCheckLaneVariant {
    /// GitHub-hosted Linux x64 copy.
    Hosted,
    /// Repository Scale Set Linux x64 copy.
    ScaleSet,
}

/// Derive each named check's exact emitted job and report identities.
///
/// The same result drives workflow emission and the event-time plan, so
/// paired lanes cannot share matrix keys or report artifacts.
/// # Errors
pub fn named_check_lanes(
    ir: &WorkflowIr,
    config: &VelnorConfig,
    dispatch: Option<ExecutionMode>,
) -> Result<BTreeMap<String, Vec<NamedCheckLane>>, ContractError> {
    let mut result = BTreeMap::new();
    if config.schema != 2 {
        for (id, job) in &ir.jobs {
            if job.check_runner.is_some() {
                result.insert(
                    id.clone(),
                    vec![NamedCheckLane {
                        variant: None,
                        job_id: id.clone(),
                        runs_on: Some(job.runs_on.clone()),
                    }],
                );
            }
        }
        return Ok(result);
    }
    let execution = config
        .execution
        .as_ref()
        .ok_or_else(|| ContractError::config("config.toml", "execution", "missing_execution"))?;
    for (id, job) in &ir.jobs {
        if job.check_runner.is_none() {
            continue;
        }
        let placement = placement_for(config, execution, dispatch, lane_class(id), id, job)?;
        let lanes = executor_lanes_for_placement(id, job, placement, execution)?;
        result.insert(id.clone(), lanes);
    }
    Ok(result)
}

/// Derive the concrete lanes for per-entry Rust and `ToFu` Qualification jobs.
///
/// `sources` maps each per-entry job to its ordinary grouped job so existing
/// profile overrides select the same runner for both job forms.
/// # Errors
pub fn qualification_job_lanes(
    ir: &WorkflowIr,
    config: &VelnorConfig,
    dispatch: Option<ExecutionMode>,
    sources: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, Vec<NamedCheckLane>>, ContractError> {
    if config.schema != 2 {
        return Ok(BTreeMap::new());
    }
    let execution = config
        .execution
        .as_ref()
        .ok_or_else(|| ContractError::config("config.toml", "execution", "missing_execution"))?;
    let mut result = BTreeMap::new();
    for (id, source_id) in sources {
        if !is_qualification_crate_job(id) {
            return Err(ContractError::identity(
                "workflow.qualification_sources",
                "unsupported_qualification_job",
            ));
        }
        let job = ir.jobs.get(id).ok_or_else(|| {
            ContractError::identity(
                "workflow.qualification_sources",
                "missing_qualification_job",
            )
        })?;
        if !ir.jobs.contains_key(source_id) {
            return Err(ContractError::identity(
                "workflow.qualification_sources",
                "missing_source_job",
            ));
        }
        let placement = placement_for_source(
            config,
            execution,
            dispatch,
            lane_class(id),
            id,
            source_id,
            job,
        )?;
        let lanes = executor_lanes_for_placement(id, job, placement, execution)?;
        result.insert(id.clone(), lanes);
    }
    for id in ir.jobs.keys().filter(|id| is_qualification_crate_job(id)) {
        if !result.contains_key(id) {
            return Err(ContractError::identity(
                "workflow.qualification_sources",
                "missing_source_mapping",
            ));
        }
    }
    Ok(result)
}

fn is_qualification_crate_job(id: &str) -> bool {
    id.starts_with("rust-qualification-") || id.starts_with("tofu-qualification-")
}

fn executor_lanes_for_placement(
    id: &str,
    job: &super::Job,
    placement: Placement,
    execution: &crate::config::ExecutionConfig,
) -> Result<Vec<NamedCheckLane>, ContractError> {
    let hosted = || NamedCheckLane {
        variant: None,
        job_id: id.to_owned(),
        runs_on: Some(job.runs_on.clone()),
    };
    let scale_set = || -> Result<NamedCheckLane, ContractError> {
        Ok(NamedCheckLane {
            variant: None,
            job_id: id.to_owned(),
            runs_on: Some(execution.scale_selector()?.token()),
        })
    };
    match placement {
        Placement::HostedOnly => Ok(vec![hosted()]),
        Placement::ScaleSetOnly => Ok(vec![scale_set()?]),
        Placement::Both => Ok(vec![
            NamedCheckLane {
                variant: Some(NamedCheckLaneVariant::Hosted),
                job_id: format!("{id}{HOSTED_SUFFIX}"),
                runs_on: Some(job.runs_on.clone()),
            },
            NamedCheckLane {
                variant: Some(NamedCheckLaneVariant::ScaleSet),
                job_id: format!("{id}{SCALE_SUFFIX}"),
                runs_on: Some(execution.scale_selector()?.token()),
            },
        ]),
    }
}

pub(super) fn add_named_check_lanes(
    ir: &mut WorkflowIr,
    lanes: &BTreeMap<String, Vec<NamedCheckLane>>,
) -> Result<(), ContractError> {
    if lanes.is_empty() {
        return Ok(());
    }
    let job = ir
        .jobs
        .get_mut(PLAN_JOB_ID)
        .ok_or_else(|| ContractError::identity("workflow.jobs", "missing_plan_for_named_checks"))?;
    let Some(step) = job.steps.iter_mut().find(|step| {
        matches!(
            &step.kind,
            StepKind::Internal { operation, .. }
                if operation == "write-request-v1:plan-v1"
        )
    }) else {
        return Err(ContractError::identity(
            "workflow.plan.steps",
            "missing_plan_request_for_named_checks",
        ));
    };
    let StepKind::Internal { env, .. } = &mut step.kind else {
        return Err(ContractError::identity(
            "workflow.plan.steps",
            "invalid_plan_request_step",
        ));
    };
    let value = serde_json::to_string(lanes).map_err(|error| {
        ContractError::identity("workflow.named_check_lanes", error.to_string())
    })?;
    if env
        .insert(NAMED_CHECK_LANES_ENV.to_owned(), value)
        .is_some()
    {
        return Err(ContractError::identity(
            "workflow.plan.steps.env",
            "duplicate_named_check_lanes",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "named_check_lanes_tests.rs"]
mod tests;
