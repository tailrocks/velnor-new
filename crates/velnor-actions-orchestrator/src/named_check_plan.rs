// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_plan::identities::platform_id_for_group;
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

pub(crate) fn derive_lanes(
    root: &Path,
    item: &DiscoveredCheck,
    run_key: &str,
    generator: &PlanGenerator,
    catalog: &ToolCatalog,
    runtime: &ApprovedMiseRuntime,
    lanes: &[NamedCheckLane],
) -> Result<(PlanObligation, Vec<MatrixEntry>), OrchestratorError> {
    derive_lanes_until(
        root,
        item,
        run_key,
        generator,
        RuntimeToolchain { catalog, runtime },
        lanes,
        None,
    )
}
pub(crate) fn derive_lanes_until(
    root: &Path,
    item: &DiscoveredCheck,
    run_key: &str,
    generator: &PlanGenerator,
    toolchain: RuntimeToolchain<'_>,
    lanes: &[NamedCheckLane],
    deadline: Option<velnor_actions_mise::CheckDeadline>,
) -> Result<(PlanObligation, Vec<MatrixEntry>), OrchestratorError> {
    if lanes.is_empty() {
        return Err(internal("named_check_lane_missing"));
    }
    let task = &item.proposal;
    let CheckPlanInputs {
        argv,
        task_digest,
        input_digest,
        closure_digest,
        planned_platform,
    } = derive_plan_inputs(root, item, generator, toolchain, deadline)?;
    let obligation = PlanObligation {
        task_id: task.task_id.clone(),
        decision: ObligationDecision::Execute,
        reason: "opaque_check_requires_execution".to_owned(),
        task_digest: task_digest.clone(),
        input_digest: input_digest.clone(),
        closure_digest,
        job_id: lanes
            .first()
            .map(|lane| lane.job_id.clone())
            .ok_or_else(|| internal("named_check_lane_missing"))?,
    };
    let run = velnor_actions_workflow_renderer::join_argv_for_run(&argv)
        .map_err(|e| internal(&e.to_string()))?;
    let mut entries = Vec::with_capacity(lanes.len());
    for lane in lanes {
        let mut entry = MatrixEntry::derive_for_lane(
            &task.stack_id,
            &task.task_id,
            &run,
            &task_digest,
            super::metadata_for(item).map_err(internal_contract)?,
            execute_ids(task),
            &input_digest,
            run_key,
            &lane.job_id,
            lane.variant,
            planned_platform.clone(),
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
    deadline: Option<velnor_actions_mise::CheckDeadline>,
) -> Result<CheckPlanInputs, OrchestratorError> {
    let task = &item.proposal;
    if toolchain.runtime.target != item.check.runner.platform.release_target() {
        return Err(internal("mise_runtime_target_mismatch"));
    }
    let argv = crate::vectors::task_argv(task, toolchain.catalog)?;
    let toolchain_inputs =
        super::toolchain_inputs(task, toolchain.catalog, toolchain.runtime)
            .map_err(internal_contract)?;
    let toolchain_id = toolchain_id(&toolchain_inputs).map_err(internal_contract)?;
    let platform =
        platform_id_for_group(&item.check.runner.label, task).map_err(internal_contract)?;
    let planned_platform = planned_platform_for_check(&item.check.runner.label, task)?;
    let graph =
        canonical_digest(&(&item.check.id, &item.check.directory)).map_err(internal_contract)?;
    let mut closure = super::resolve_closure_until(
        root,
        task,
        &graph,
        &toolchain_id,
        &platform,
        deadline,
    )
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
    let task_digest = crate::internal::plan_obligation::task_digest(
        &task.task_id,
        &argv,
        &toolchain_id,
    )
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
    let planned = PlannedPlatform::new(&task.runner_profile, &target).map_err(internal_contract)?;
    if planned.platform_id != platform_id_for_group(label, task).map_err(internal_contract)? {
        return Err(internal("planned_platform_identity_mismatch"));
    }
    Ok(planned)
}
