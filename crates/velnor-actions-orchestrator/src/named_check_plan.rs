// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_plan::snapshot::canonical_digest;
use crate::internal_plan::snapshot::platform_target_for_group;
use crate::internal_plan::{IdentityInputs, execute_ids, task_identity_digest};
use std::path::Path;
use velnor_actions_contract::cachekey::toolchain_id;
use velnor_actions_contract::{
    MatrixEntry, NamedCheckLane, ObligationDecision, PlanGenerator, PlanObligation, PlannedPlatform,
};
use velnor_actions_mise::catalog::lock::ApprovedMiseRuntime;
use velnor_actions_mise::{DiscoveredCheck, ToolCatalog};

#[derive(Clone, Copy)]
pub(crate) struct RuntimeToolchain<'a> {
    pub(crate) catalog: &'a ToolCatalog,
    pub(crate) runtime: &'a ApprovedMiseRuntime,
}

#[derive(Clone, Copy)]
pub(crate) struct CheckLanes<'a> {
    pub(crate) lanes: &'a [NamedCheckLane],
    pub(crate) routes: &'a [velnor_actions_contract::LaneRoute],
}

pub(crate) fn derive_lanes(
    root: &Path,
    item: &DiscoveredCheck,
    run_key: &str,
    generator: &PlanGenerator,
    toolchain: RuntimeToolchain<'_>,
    selection: CheckLanes<'_>,
) -> Result<(PlanObligation, Vec<MatrixEntry>), OrchestratorError> {
    derive_lanes_until(root, item, run_key, generator, toolchain, selection, None)
}
pub(crate) fn derive_lanes_until(
    root: &Path,
    item: &DiscoveredCheck,
    run_key: &str,
    generator: &PlanGenerator,
    toolchain: RuntimeToolchain<'_>,
    selection: CheckLanes<'_>,
    deadline: Option<velnor_actions_mise::CheckDeadline>,
) -> Result<(PlanObligation, Vec<MatrixEntry>), OrchestratorError> {
    let CheckLanes { lanes, routes } = selection;
    if lanes.is_empty() {
        return Err(internal("named_check_lane_missing"));
    }
    let task = &item.proposal;
    let baseline = derive_plan_inputs(
        root,
        item,
        generator,
        toolchain,
        &item.check.runner.label,
        deadline,
    )?;
    let mut obligation = PlanObligation {
        task_id: task.task_id.clone(),
        decision: ObligationDecision::Execute,
        reason: "opaque_check_requires_execution".to_owned(),
        task_digest: baseline.task_digest.clone(),
        input_digest: baseline.input_digest.clone(),
        closure_digest: baseline.closure_digest.clone(),
        job_id: if lanes.len() == 2 {
            format!("check-{}", item.check.id)
        } else {
            lanes
                .first()
                .map(|lane| lane.job_id.clone())
                .ok_or_else(|| internal("named_check_lane_missing"))?
        },
    };
    let mut entries = Vec::with_capacity(lanes.len());
    for lane in lanes {
        let values = if let Some(route) = routes.iter().find(|route| route.job_id == lane.job_id) {
            if route.variant != lane.variant
                || route.target.as_deref() != Some(task.identity.target.as_str())
                || !route.task_ids.contains(&task.task_id)
            {
                return Err(internal("named_check_route_mismatch"));
            }
            derive_plan_inputs(root, item, generator, toolchain, &route.runner, deadline)?
        } else if lane.variant.is_none() {
            baseline.clone()
        } else {
            return Err(internal("named_check_route_missing"));
        };
        if lanes.len() == 1 {
            obligation.input_digest.clone_from(&values.input_digest);
            obligation.closure_digest.clone_from(&values.closure_digest);
        }
        let run = velnor_actions_workflow_renderer::join_argv_for_run(&values.argv)
            .map_err(|e| internal(&e.to_string()))?;
        let mut entry = MatrixEntry::derive_for_lane(
            &task.stack_id,
            &task.task_id,
            &run,
            &values.task_digest,
            super::metadata_for(item).map_err(internal_contract)?,
            execute_ids(task),
            &values.input_digest,
            run_key,
            &lane.job_id,
            lane.variant,
            values.planned_platform,
        )
        .map_err(internal_contract)?;
        entry.declared_outputs = item
            .check
            .evidence
            .iter()
            .map(|evidence| evidence.path.clone())
            .collect();
        entries.push(entry);
    }
    Ok((obligation, entries))
}

#[derive(Clone)]
struct CheckPlanInputs {
    argv: Vec<String>,
    task_digest: String,
    input_digest: String,
    closure_digest: String,
    planned_platform: PlannedPlatform,
}

fn derive_plan_inputs(
    root: &Path,
    item: &DiscoveredCheck,
    generator: &PlanGenerator,
    toolchain: RuntimeToolchain<'_>,
    runner: &str,
    deadline: Option<velnor_actions_mise::CheckDeadline>,
) -> Result<CheckPlanInputs, OrchestratorError> {
    let task = &item.proposal;
    if toolchain.runtime.target != item.check.runner.platform.release_target() {
        return Err(internal("mise_runtime_target_mismatch"));
    }
    let argv = crate::vectors::task_argv(task, toolchain.catalog)?;
    let toolchain_inputs = super::toolchain_inputs(task, toolchain.catalog, toolchain.runtime)
        .map_err(internal_contract)?;
    let toolchain_id = toolchain_id(&toolchain_inputs).map_err(internal_contract)?;
    let planned_platform = planned_platform_for_check(runner, task)?;
    let platform = planned_platform.platform_id.clone();
    let graph =
        canonical_digest(&(&item.check.id, &item.check.directory)).map_err(internal_contract)?;
    let mut closure =
        super::resolve_closure_until(root, task, &graph, &toolchain_id, &platform, deadline)
            .map_err(internal_contract)?;
    closure.inputs.insert(
        "task_definition".to_owned(),
        velnor_actions_contract::Provenance::Known {
            digest: canonical_digest(&item.check).map_err(internal_contract)?,
        },
    );
    closure.inputs.insert(
        "resolved_task_config".to_owned(),
        velnor_actions_contract::Provenance::Known {
            digest: velnor_actions_contract::digest_b3(item.task_config.as_bytes()),
        },
    );
    let closure_digest = canonical_digest(&closure).map_err(internal_contract)?;
    let input_digest = task_identity_digest(&IdentityInputs {
        task,
        argv: &argv,
        toolchain_id: &toolchain_id,
        platform_id: &platform,
        manifest: &task.identity.unit_path,
        generator,
        extension: super::extension_for(item).map_err(internal_contract)?,
        closure_digest: &closure_digest,
    })
    .map_err(internal_contract)?;
    let task_digest =
        crate::internal::plan_obligation::task_digest(&task.task_id, &argv, &toolchain_id)
            .map_err(internal_contract)?;
    Ok(CheckPlanInputs {
        argv,
        task_digest,
        input_digest,
        closure_digest,
        planned_platform,
    })
}

fn planned_platform_for_check(
    label: &str,
    task: &velnor_actions_contract::ProposedTask,
) -> Result<PlannedPlatform, OrchestratorError> {
    let target = platform_target_for_group(label, task).map_err(internal_contract)?;
    PlannedPlatform::new(label, &target).map_err(internal_contract)
}
