// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::super::ir::{Job, StepKind, WorkflowIr};
use super::{LaneClass, Placement};
use crate::config::BUN_JOB_PREFIX;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

pub const LANE_INVENTORY_ENV: &str = "VELNOR_LANE_INVENTORY_JSON";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneVariant {
    Hosted,
    ScaleSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneRoute {
    pub logical_job_id: String,
    pub job_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<LaneVariant>,
    pub runner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub task_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneInventory {
    pub mode: Option<crate::config::ExecutionMode>,
    pub routes: Vec<LaneRoute>,
}

#[derive(Debug, Clone)]
pub struct LanePlan {
    routes: BTreeMap<String, (LaneClass, Placement)>,
    id_map: BTreeMap<String, Vec<String>>,
    reverse: BTreeMap<String, String>,
    concrete: BTreeMap<String, LaneRoute>,
    scale_token: Option<String>,
    mode: Option<crate::config::ExecutionMode>,
}

impl LanePlan {
    pub(super) fn from_placements(
        ir: &WorkflowIr,
        placements: &BTreeMap<String, (LaneClass, Placement)>,
        id_map: &BTreeMap<String, Vec<String>>,
        scale_token: Option<String>,
        scale_target: Option<&str>,
        task_bindings: &BTreeMap<String, Vec<String>>,
        mode: Option<crate::config::ExecutionMode>,
    ) -> Result<Self, ContractError> {
        if task_bindings.keys().any(|id| !ir.jobs.contains_key(id)) {
            return Err(plan_error("unknown_task_binding_job", ""));
        }
        let mut routes = BTreeMap::new();
        for (logical_id, (class, placement)) in placements {
            if logical_id.starts_with(BUN_JOB_PREFIX) && *placement == Placement::Both {
                return Err(plan_error("paired_bun_placement", logical_id));
            }
            routes.insert(logical_id.clone(), (*class, *placement));
        }
        let mut reverse = BTreeMap::new();
        let mut concrete = BTreeMap::new();
        for (logical, outputs) in id_map {
            let job = ir
                .jobs
                .get(logical)
                .ok_or_else(|| plan_error("unknown_logical_id", logical))?;
            let (_, placement) = placements
                .get(logical)
                .ok_or_else(|| plan_error("unknown_logical_id", logical))?;
            let task_ids = route_tasks(job, *placement, logical, task_bindings.get(logical))?;
            for (index, output) in outputs.iter().enumerate() {
                if reverse.insert(output.clone(), logical.clone()).is_some() {
                    return Err(plan_error("duplicate_output_id", output));
                }
                let variant = match placement {
                    Placement::Both if index == 0 => Some(LaneVariant::Hosted),
                    Placement::Both if index == 1 => Some(LaneVariant::ScaleSet),
                    Placement::HostedOnly | Placement::ScaleSetOnly if index == 0 => None,
                    _ => return Err(plan_error("invalid_route_count", logical)),
                };
                let runner = match (placement, variant) {
                    (Placement::ScaleSetOnly, _)
                    | (Placement::Both, Some(LaneVariant::ScaleSet)) => scale_token
                        .as_ref()
                        .ok_or_else(|| plan_error("missing_scale_runner", logical))?
                        .clone(),
                    _ => job.runs_on.clone(),
                };
                let target = match (placement, variant) {
                    (Placement::ScaleSetOnly, _)
                    | (Placement::Both, Some(LaneVariant::ScaleSet)) => {
                        scale_target.map(str::to_owned)
                    }
                    _ => crate::targets::ReleaseTarget::for_runner_label(&runner)
                        .map(|target| target.triple().to_owned()),
                };
                concrete.insert(
                    output.clone(),
                    LaneRoute {
                        logical_job_id: logical.clone(),
                        job_id: output.clone(),
                        variant,
                        runner,
                        target,
                        task_ids: task_ids.clone(),
                    },
                );
            }
            if outputs.len() != if *placement == Placement::Both { 2 } else { 1 } {
                return Err(plan_error("invalid_route_count", logical));
            }
        }
        Ok(Self {
            routes,
            id_map: id_map.clone(),
            reverse,
            concrete,
            scale_token,
            mode,
        })
    }

    #[must_use]
    pub fn route_for_output(&self, output_id: &str) -> Option<&LaneRoute> {
        self.concrete.get(output_id)
    }

    #[must_use]
    pub fn paired_routes(&self) -> Vec<LaneRoute> {
        self.concrete
            .values()
            .filter(|route| route.variant.is_some() && self.is_task_verification(route))
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn inventory(&self) -> LaneInventory {
        LaneInventory {
            mode: self.mode,
            routes: self
                .concrete
                .values()
                .filter(|route| self.is_task_verification(route))
                .cloned()
                .collect(),
        }
    }

    fn has_scale_tasks(&self) -> bool {
        self.concrete.values().any(|route| {
            self.is_task_verification(route)
                && self
                    .routes
                    .get(&route.logical_job_id)
                    .is_some_and(|(_, placement)| {
                        matches!(placement, Placement::ScaleSetOnly | Placement::Both)
                    })
        })
    }

    fn is_task_verification(&self, route: &LaneRoute) -> bool {
        !route.task_ids.is_empty()
            && self
                .routes
                .get(&route.logical_job_id)
                .is_some_and(|(class, _)| *class == LaneClass::Verification)
    }

    #[must_use]
    pub fn logical_id_for_output(&self, output_id: &str) -> Option<&str> {
        self.reverse.get(output_id).map(String::as_str)
    }

    pub fn output_ids(&self, logical_id: &str) -> Result<Vec<String>, ContractError> {
        self.route(logical_id)?;
        self.id_map
            .get(logical_id)
            .cloned()
            .ok_or_else(|| plan_error("unknown_logical_id", logical_id))
    }

    pub fn project(
        &self,
        logical_id: &str,
        canonical_job: &Job,
    ) -> Result<Vec<(String, Job)>, ContractError> {
        let (class, placement) = self.route(logical_id)?;
        let projected = super::emit_job_variants(
            logical_id,
            canonical_job,
            self.scale_token.as_deref(),
            *class,
            *placement,
            &self.id_map,
        )?;
        if logical_id.starts_with(BUN_JOB_PREFIX)
            && (projected.len() != 1 || projected[0].0 != logical_id)
        {
            return Err(plan_error("bun_output_id_mismatch", logical_id));
        }
        Ok(projected)
    }

    fn route(&self, logical_id: &str) -> Result<&(LaneClass, Placement), ContractError> {
        self.routes
            .get(logical_id)
            .ok_or_else(|| plan_error("unknown_logical_id", logical_id))
    }
}

pub(super) fn add_lane_routes(
    ir: &mut WorkflowIr,
    plan: &LanePlan,
    include_empty: bool,
) -> Result<(), ContractError> {
    let routes = plan.paired_routes();
    if routes.is_empty() && !include_empty && !plan.has_scale_tasks() {
        return Ok(());
    }
    let value = serde_json::to_string(&plan.inventory())
        .map_err(|error| ContractError::identity("workflow.lane_routes", error.to_string()))?;
    for (job_id, operation) in [
        (super::super::jobs::PLAN_JOB_ID, "write-request-v1:plan-v1"),
        (
            super::super::jobs::REQUIRED_JOB_ID,
            "write-request-v1:merge-v1",
        ),
    ] {
        let job = ir.jobs.get_mut(job_id).ok_or_else(|| {
            ContractError::identity("workflow.lane_routes", format!("missing_job:{job_id}"))
        })?;
        let env = job
            .steps
            .iter_mut()
            .find_map(|step| match &mut step.kind {
                StepKind::Internal {
                    operation: actual,
                    env,
                } if actual == operation => Some(env),
                _ => None,
            })
            .ok_or_else(|| {
                ContractError::identity("workflow.lane_routes", format!("missing_request:{job_id}"))
            })?;
        if env
            .insert(LANE_INVENTORY_ENV.to_owned(), value.clone())
            .is_some()
        {
            return Err(ContractError::identity(
                "workflow.lane_routes",
                "duplicate_routes",
            ));
        }
    }
    Ok(())
}

fn route_tasks(
    job: &Job,
    placement: Placement,
    logical: &str,
    binding: Option<&Vec<String>>,
) -> Result<Vec<String>, ContractError> {
    let mut task_ids = std::collections::BTreeSet::new();
    for step in &job.steps {
        if let StepKind::TaskExecution { task_id, .. } = &step.kind {
            task_ids.insert(task_id.clone());
        }
    }
    if job.check_runner.is_some() {
        match super::super::named_check_lanes::named_check_task_id(job) {
            Ok(task) => {
                task_ids.insert(task);
            }
            Err(error) if placement == Placement::Both => return Err(error),
            Err(_) => {}
        }
    }
    if let Some(bound) = binding {
        let mut supplied = std::collections::BTreeSet::new();
        for id in bound {
            crate::ids::validate_task_id(id)?;
            if !supplied.insert(id.clone()) {
                return Err(plan_error("duplicate_task_binding", logical));
            }
        }
        if !task_ids.is_empty() && task_ids != supplied {
            return Err(plan_error("task_binding_mismatch", logical));
        }
        task_ids = supplied;
    }
    if placement == Placement::Both
        && task_ids.iter().any(|id| {
            id.starts_with("stack/rust/")
                && crate::ids::split_shard_suffix(id).is_some_and(|(_, _, count)| count > 1)
        })
    {
        return Err(plan_error("paired_rust_sharding_unsupported", logical));
    }
    Ok(task_ids.into_iter().collect())
}

fn plan_error(problem: &str, logical_id: &str) -> ContractError {
    ContractError::identity("lane_plan", format!("{problem}:{logical_id}"))
}
