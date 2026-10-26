// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::super::ir::{Job, StepKind, WorkflowIr};
use super::super::jobs::PLAN_JOB_ID;
use super::bun_plan::BunLanePlan;
use crate::config::BUN_JOB_PREFIX;
use crate::errors::ContractError;

pub const BUN_JOB_LANES_ENV: &str = "VELNOR_BUN_JOB_LANES_JSON";

impl BunLanePlan {
    #[must_use]
    pub fn job_lanes(&self) -> BTreeMap<String, Vec<String>> {
        self.routes
            .iter()
            .filter(|(logical_id, _)| logical_id.starts_with(BUN_JOB_PREFIX))
            .map(|(logical_id, route)| {
                (
                    logical_id.clone(),
                    route
                        .outputs
                        .iter()
                        .map(|output| output.id.clone())
                        .collect(),
                )
            })
            .collect()
    }
}

pub(super) fn identity_job_lanes(jobs: &BTreeMap<String, Job>) -> BTreeMap<String, Vec<String>> {
    jobs.keys()
        .filter(|job_id| job_id.starts_with(BUN_JOB_PREFIX))
        .map(|job_id| (job_id.clone(), vec![job_id.clone()]))
        .collect()
}

pub(super) fn add_bun_job_lanes(
    ir: &mut WorkflowIr,
    lanes: &BTreeMap<String, Vec<String>>,
) -> Result<(), ContractError> {
    if lanes.is_empty() {
        return Ok(());
    }
    let job = ir
        .jobs
        .get_mut(PLAN_JOB_ID)
        .ok_or_else(|| ContractError::identity("workflow.jobs", "missing_plan_for_bun_lanes"))?;
    let Some(step) = job.steps.iter_mut().find(|step| {
        matches!(
            &step.kind,
            StepKind::Internal { operation, .. }
                if operation == "write-request-v1:plan-v1"
        )
    }) else {
        return Err(ContractError::identity(
            "workflow.plan.steps",
            "missing_plan_request_for_bun_lanes",
        ));
    };
    let StepKind::Internal { env, .. } = &mut step.kind else {
        return Err(ContractError::identity(
            "workflow.plan.steps",
            "invalid_plan_request_step",
        ));
    };
    let value = serde_json::to_string(lanes)
        .map_err(|error| ContractError::identity("workflow.bun_job_lanes", error.to_string()))?;
    if env.insert(BUN_JOB_LANES_ENV.to_owned(), value).is_some() {
        return Err(ContractError::identity(
            "workflow.plan.steps.env",
            "duplicate_bun_job_lanes",
        ));
    }
    Ok(())
}
