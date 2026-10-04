//! Per-obligation identities and changed-work dispositions (P02).
//!
//! Every universe member gets content identities first; the changed-work
//! classification then assigns exactly one disposition: changed
//! obligations execute unconditionally, while unchanged ones take the
//! reuse outcome (execute until task-cache proof exists) and stay
//! eligible for later baseline coverage.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_actions_contract::{MatrixEntry, PlanObligation, ProposedTask};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::restore::probe_tool_availability;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::internal::{internal, internal_contract};
use crate::internal_plan::snapshot::ExecutionSnapshot;
use crate::internal_plan::wire_w2::{self, GroupWire};
use crate::internal_plan::{
    adapter_metadata, cache_ids_for, evidence_for_group, execute_ids, record_task_cache,
};
use crate::schedule::assign_lanes;
use crate::select::group_changed;
#[path = "source_identity.rs"]
pub(crate) mod source_identity;
use source_identity::{ResolvedSourceIdentity, SourceIdentityInputs, extension_for_task};

#[cfg(test)]
#[path = "plan_obligation_tests.rs"]
mod tests;

/// Inputs for planning one obligation.
pub(crate) struct GroupInputs<'a> {
    /// Validated discovery inventory.
    pub(crate) discovery: &'a Discovery,
    /// Universe member to plan.
    pub(crate) task: &'a ProposedTask,
    /// Run key.
    pub(crate) run_key: &'a str,
    /// Runner label.
    pub(crate) label: &'a str,
    /// Pinned tool catalog.
    pub(crate) catalog: &'a ToolCatalog,
    /// Reuse wiring inputs.
    pub(crate) wire: GroupWire<'a>,
    /// Changed-work classification for the group.
    pub(crate) changed: bool,
    /// Once-built execution snapshot for graph lookups (P03-1).
    pub(crate) snapshot: &'a ExecutionSnapshot,
    /// Repository checkout the closure resolves against.
    pub(crate) root: &'a Path,
}

/// Deterministic lane per universe task ID.
pub(crate) fn lane_table(universe: &[&ProposedTask]) -> BTreeMap<String, u32> {
    let ids: Vec<String> = universe.iter().map(|task| task.task_id.clone()).collect();
    assign_lanes(&ids).into_iter().collect()
}

/// Changed unit keys for tasks with empty unit IDs.
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

/// True when one universe member counts as changed.
pub(crate) fn member_changed(
    task: &ProposedTask,
    changed: Option<&crate::select::ChangedSelection>,
    keys: &BTreeSet<String>,
) -> bool {
    changed.is_none_or(|set| group_changed(task, &set.affected, keys))
}

/// Obligation plus matrix entry for one universe member.
///
/// Identities attach first; changed members execute unconditionally
/// while unchanged members take the reuse outcome for later baseline
/// classification.
pub(crate) fn plan_group(
    inputs: &GroupInputs<'_>,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<(PlanObligation, MatrixEntry), OrchestratorError> {
    let task = inputs.task;
    let identity = source_identity::resolve(
        &SourceIdentityInputs {
            discovery: inputs.discovery,
            task,
            root: inputs.root,
            snapshot: inputs.snapshot,
            catalog: inputs.catalog,
            generator: inputs.wire.generator,
            label: inputs.label,
        },
        reads,
    )?;
    let reuse = planned_reuse(
        inputs,
        &identity,
        &identity.toolchain_id,
        &identity.platform_id,
        reads,
    )?;
    let argv = identity.argv;
    let toolchain = identity.toolchain_id;
    let input_digest = identity.input_digest;
    let closure_digest = identity.closure_digest;
    let task_digest = identity.task_digest;
    // The persisted input digest flows through the validated reuse
    // outcome when one exists, so merge-time live comparison judges the
    // exact recorded value; forced-execution paths carry the identity.
    let recorded = reuse
        .recorded_input_digest
        .clone()
        .unwrap_or_else(|| input_digest.clone());
    let job_id = crate::crate_job_ids::job_id_for_member(&inputs.discovery.proposals, task)
        .ok_or_else(|| internal("crate_job_id_missing"))?;
    let obligation = PlanObligation {
        task_id: task.task_id.clone(),
        job_id: job_id.clone(),
        decision: reuse.decision,
        reason: reuse.reason,
        task_digest: task_digest.clone(),
        input_digest: recorded,
        closure_digest,
        execution_identity: identity.execution_identity,
        baseline_proof: None,
    };
    let mut metadata = adapter_metadata(task, evidence_for_group(inputs.discovery, task))
        .map_err(internal_contract)?;
    if let Some(helper) = identity.helper_obligation {
        metadata["helper_obligation"] = serde_json::to_value(helper)
            .map_err(|error| internal(&format!("helper_descriptor_serialization:{error}")))?;
    }
    record_task_cache(
        &mut metadata,
        reuse.task_cache_enabled,
        reuse.task_cache_key.as_deref(),
    );
    let run = velnor_actions_workflow_renderer::join_argv_for_run(&argv)
        .map_err(|err| internal(&err.to_string()))?;
    let mut entry = MatrixEntry::derive(
        &task.stack_id,
        &task.task_id,
        &run,
        &task_digest,
        metadata,
        execute_ids(task),
        &input_digest,
        inputs.run_key,
        &job_id,
    )
    .map_err(internal_contract)?;
    let cache_ids = cache_ids_for(task, inputs.label, &toolchain).map_err(internal_contract)?;
    record_lane_target_dir(&mut entry.adapter_metadata, cache_ids.lane_id());
    entry.cache_ids = Some(cache_ids);
    entry.native_recipe = identity.native_recipe;
    Ok((obligation, entry))
}

/// Reuse disposition from the task's source-bound identity, never a pathname alone.
fn planned_reuse(
    inputs: &GroupInputs<'_>,
    identity: &ResolvedSourceIdentity,
    toolchain: &str,
    platform: &str,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<wire_w2::ReuseOutcome, OrchestratorError> {
    let task = inputs.task;
    let (_, eligible) = extension_for_task(task, inputs.root, &identity.bundle, reads)?;
    let reuse = if inputs.changed {
        wire_w2::ReuseOutcome::execute("affected_by_change")
    } else {
        wire_w2::plan_reuse_outcome(
            task,
            inputs.wire.event,
            probe_tool_availability(false, false),
            toolchain,
            &identity.input_digest,
            eligible,
        )?
    };
    let gate = wire_w2::check_archive_identity_with_source(
        task,
        toolchain,
        platform,
        identity.bundle.config_digest(),
        Some(&identity.closure_digest),
    )?;
    Ok(match gate {
        wire_w2::ArchiveGate::SourceUnbound if !inputs.changed => {
            wire_w2::ReuseOutcome::execute("archive_source_unbound")
        }
        _ => reuse,
    })
}

/// Record the lane's isolated `CARGO_TARGET_DIR` on entry metadata.
///
/// Generated legs read this path for their step env; the value derives
/// from the responsibility-based lane digest via `target_dir_for_lane`.
fn record_lane_target_dir(metadata: &mut serde_json::Value, lane_id: &str) {
    let Some(object) = metadata.as_object_mut() else {
        return;
    };
    object.insert(
        "cargo_target_dir".to_owned(),
        serde_json::Value::String(crate::internal_plan::target_dir_for_lane_id(lane_id)),
    );
}

/// Task digest binding argv plus toolchain for one obligation.
///
/// Shared by event-time plan obligations and static crate-job
/// obligations so both judge the same digest.
pub(crate) fn task_digest(
    task_id: &str,
    argv: &[String],
    toolchain_id: &str,
    helper_obligation: Option<&velnor_actions_contract::HelperObligationDescriptor>,
    native_recipe: Option<&velnor_actions_contract::NativeValidationDescriptor>,
) -> Result<String, velnor_actions_contract::ContractError> {
    velnor_actions_contract::canonical_task_digest(
        task_id,
        argv,
        toolchain_id,
        helper_obligation,
        native_recipe,
    )
}
