//! Exact named-check job/report identities shared by workflow and plan.

use super::ir::WorkflowIr;
use super::jobs::PLAN_JOB_ID;
use super::lanes::{Placement, placement_for};
use super::step::StepKind;
use super::{HOSTED_SUFFIX, SCALE_SUFFIX, lane_class};
use crate::config::{ExecutionMode, VelnorConfig};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Env key carrying the static named-check output identities into `plan-v1`.
pub const NAMED_CHECK_LANES_ENV: &str = "VELNOR_NAMED_CHECK_LANES_JSON";
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
        let lanes = match placement_for(config, execution, dispatch, lane_class(id), id, job)? {
            Placement::HostedOnly | Placement::ScaleSetOnly => vec![NamedCheckLane {
                variant: None,
                job_id: id.clone(),
            }],
            Placement::Both => vec![
                NamedCheckLane {
                    variant: Some(NamedCheckLaneVariant::Hosted),
                    job_id: format!("{id}{HOSTED_SUFFIX}"),
                },
                NamedCheckLane {
                    variant: Some(NamedCheckLaneVariant::ScaleSet),
                    job_id: format!("{id}{SCALE_SUFFIX}"),
                },
            ],
        };
        result.insert(id.clone(), lanes);
    }
    Ok(result)
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
