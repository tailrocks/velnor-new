// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use velnor_actions_contract::{
    MatrixEntry, NamedCheckLane, PlanObligation, PlannedPlatform, ProposedTask, Stack,
    canonical_json_bytes, digest_b3,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::restore::probe_tool_availability;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::internal::{internal, internal_contract};
use crate::internal_plan::identities::platform_id_for_group;
use crate::internal_plan::snapshot::{ExecutionSnapshot, platform_target_for_group};
use crate::internal_plan::wire_w2::{self, GroupWire};
use crate::internal_plan::{
    adapter_metadata, cache_ids_for, evidence_for_group, execute_ids, record_task_cache,
    toolchain_id,
};
use crate::schedule::assign_lanes;
use crate::select::group_changed;

#[path = "plan_obligation_identity.rs"]
mod identity;

use identity::planned_identity;

pub(crate) struct GroupInputs<'a> {
    pub(crate) discovery: &'a Discovery,
    pub(crate) job_owners: &'a crate::crate_job_ids::JobOwners<'a>,
    pub(crate) task: &'a ProposedTask,
    pub(crate) run_key: &'a str,
    pub(crate) label: &'a str,
    pub(crate) lane: u32,
    pub(crate) catalog: &'a ToolCatalog,
    pub(crate) wire: GroupWire<'a>,
    pub(crate) changed: bool,
    pub(crate) snapshot: &'a ExecutionSnapshot,
    pub(crate) root: &'a Path,
    pub(crate) named_check_lanes: &'a BTreeMap<String, Vec<NamedCheckLane>>,
}

pub(crate) fn lane_table(universe: &[&ProposedTask]) -> BTreeMap<String, u32> {
    let ids: Vec<String> = universe.iter().map(|task| task.task_id.clone()).collect();
    assign_lanes(&ids).into_iter().collect()
}

pub(crate) fn changed_keys(
    universe: &[&ProposedTask],
    changed: &BTreeSet<String>,
) -> BTreeSet<String> {
    universe
        .iter()
        .filter(|task| changed.contains(&task.identity.unit_id))
        .map(|task| task.identity.unit_key.clone())
        .collect()
}

pub(crate) fn member_changed(
    task: &ProposedTask,
    changed: Option<&BTreeSet<String>>,
    keys: &BTreeSet<String>,
) -> bool {
    changed.is_none_or(|set| group_changed(task, set, keys))
}

pub(crate) fn plan_group(
    inputs: &GroupInputs<'_>,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<(PlanObligation, Vec<MatrixEntry>), OrchestratorError> {
    let task = inputs.task;
    if Stack::from_id(&task.stack_id) == Some(Stack::Native) {
        let check = inputs
            .discovery
            .native_checks
            .iter()
            .find(|check| task.task_id == format!("internal/native-check/{}", check.id))
            .ok_or_else(|| internal("native_check_not_configured"))?;
        return crate::internal_plan::native_homebrew_checks::derive(
            inputs.root,
            check,
            task,
            inputs.run_key,
            inputs.wire.generator,
            inputs.wire.event,
        );
    }
    if Stack::from_id(&task.stack_id) == Some(Stack::Mise) {
        let item = crate::internal_plan::named_checks::discovered(inputs.discovery, task)
            .map_err(internal_contract)?;
        let job_id = format!("check-{}", item.check.id);
        let check_lanes = inputs
            .named_check_lanes
            .get(&job_id)
            .ok_or_else(|| internal("named_check_lane_missing"))?;
        return crate::internal_plan::named_checks::plan::derive_lanes(
            inputs.root,
            item,
            inputs.run_key,
            inputs.wire.generator,
            inputs.catalog,
            check_lanes,
        );
    }
    let _ = inputs.lane;
    let toolchain = toolchain_id(task, inputs.catalog).map_err(internal_contract)?;
    let (argv, binding_toolchain) =
        crate::tool_catalog_project::task_binding(task, inputs.catalog)?;
    let platform_id = platform_id_for_group(inputs.label, task).map_err(internal_contract)?;
    let identity = planned_identity(inputs, &argv, &toolchain, &platform_id, &mut *reads)?;
    let reuse_eligible = identity.reuse_eligible;
    let input_digest = identity.input_digest;
    let closure_digest = identity.closure_digest;
    let gate = wire_w2::check_archive_identity_with_source(
        task,
        &toolchain,
        &platform_id,
        identity.bundle.config_digest(),
        Some(&closure_digest),
        inputs.catalog,
    )?;
    let reuse = if inputs.changed {
        wire_w2::ReuseOutcome::execute("affected_by_change")
    } else if gate == wire_w2::ArchiveGate::SourceUnbound {
        return Err(internal(&format!(
            "archive_source_unbound:{}",
            task.task_id
        )));
    } else if Stack::from_id(&task.stack_id) == Some(Stack::Rust) {
        crate::plan_dispositions::unaffected_rust_outcome(task)
    } else {
        wire_w2::plan_reuse_outcome(
            task,
            inputs.wire.event,
            probe_tool_availability(false, false),
            &toolchain,
            &input_digest,
            reuse_eligible,
        )?
    };
    complete_group(
        inputs,
        &argv,
        &binding_toolchain,
        &input_digest,
        closure_digest,
        reuse,
    )
    .map(|(obligation, entry)| (obligation, vec![entry]))
}

fn complete_group(
    inputs: &GroupInputs<'_>,
    argv: &[String],
    toolchain: &str,
    input_digest: &str,
    closure_digest: String,
    reuse: wire_w2::ReuseOutcome,
) -> Result<(PlanObligation, MatrixEntry), OrchestratorError> {
    let task = inputs.task;
    let task_digest = task_digest(&task.task_id, argv, toolchain).map_err(internal_contract)?;
    let recorded = reuse
        .recorded_input_digest
        .clone()
        .unwrap_or_else(|| input_digest.to_owned());
    let Some(job_id) = inputs.job_owners.for_member(task)? else {
        return Err(internal("crate_job_id_missing"));
    };
    let obligation = PlanObligation {
        task_id: task.task_id.clone(),
        decision: reuse.decision,
        reason: reuse.reason,
        task_digest: task_digest.clone(),
        input_digest: recorded,
        closure_digest,
        job_id: job_id.clone(),
    };
    let mut metadata = adapter_metadata(task, evidence_for_group(inputs.discovery, task))
        .map_err(internal_contract)?;
    record_task_cache(
        &mut metadata,
        reuse.task_cache_enabled,
        reuse.task_cache_key.as_deref(),
    );
    let run = velnor_actions_workflow_renderer::join_argv_for_run(argv)
        .map_err(|err| internal(&err.to_string()))?;
    let planned_platform = planned_platform_for_group(inputs.label, task)?;
    let mut entry = MatrixEntry::derive(
        &task.stack_id,
        &task.task_id,
        &run,
        &task_digest,
        metadata,
        execute_ids(task),
        input_digest,
        inputs.run_key,
        &job_id,
        planned_platform,
    )
    .map_err(internal_contract)?;
    let cache_ids =
        cache_ids_for(task, inputs.label, toolchain, inputs.catalog).map_err(internal_contract)?;
    record_lane_target_dir(&mut entry.adapter_metadata, cache_ids.lane_id());
    entry.cache_ids = Some(cache_ids);
    Ok((obligation, entry))
}

fn planned_platform_for_group(
    label: &str,
    task: &ProposedTask,
) -> Result<PlannedPlatform, OrchestratorError> {
    let target = platform_target_for_group(label, task).map_err(internal_contract)?;
    let planned = PlannedPlatform::new(label, &target).map_err(internal_contract)?;
    if planned.platform_id != platform_id_for_group(label, task).map_err(internal_contract)? {
        return Err(internal("planned_platform_identity_mismatch"));
    }
    Ok(planned)
}

fn record_lane_target_dir(metadata: &mut serde_json::Value, lane_id: &str) {
    let Some(object) = metadata.as_object_mut() else {
        return;
    };
    object.insert(
        "cargo_target_dir".to_owned(),
        serde_json::Value::String(crate::internal_plan::target_dir_for_lane_id(lane_id)),
    );
}

fn digest_of<T: Serialize>(inputs: &T) -> Result<String, velnor_actions_contract::ContractError> {
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}

pub(crate) fn task_digest(
    task_id: &str,
    argv: &[String],
    toolchain_id: &str,
) -> Result<String, velnor_actions_contract::ContractError> {
    digest_of(&TaskDigestInputs {
        task_id,
        argv,
        toolchain_id,
    })
}

#[derive(Debug, Serialize)]
struct TaskDigestInputs<'a> {
    task_id: &'a str,
    argv: &'a [String],
    toolchain_id: &'a str,
}
