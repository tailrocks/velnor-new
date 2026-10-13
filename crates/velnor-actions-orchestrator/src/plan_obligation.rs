//! Per-obligation identities and changed-work dispositions (P02).
//!
//! Every universe member gets content identities first; the changed-work
//! classification then assigns exactly one disposition: changed
//! obligations execute unconditionally, while unchanged ones take the
//! reuse outcome (execute until task-cache proof exists) and stay
//! eligible for later baseline coverage.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_actions_contract::{
    MatrixEntry, NamedCheckLane, PlanObligation, PlannedPlatform, ProposedTask, Stack,
    StackExtension,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::restore::probe_tool_availability;
use velnor_actions_rust::extension_for_proposal;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::internal::{internal, internal_contract};
use crate::internal_plan::closure::resolve_closure_at_root;
use crate::internal_plan::identities::{
    ExtensionBundle, extension_bundle_with_snapshot, platform_id_for_group,
};
use crate::internal_plan::snapshot::{
    ExecutionSnapshot, canonical_digest, platform_target_for_group,
};
use crate::internal_plan::wire_w2::{self, GroupWire};
use crate::internal_plan::{
    IdentityInputs, adapter_metadata, cache_ids_for, evidence_for_group, execute_ids,
    nextest_config_for, record_task_cache, task_identity_digest, toolchain_id,
};
use crate::schedule::assign_lanes;
use crate::select::group_changed;
use crate::vectors::task_argv;

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
    /// Deprecated ordinal lane from the caller; ignored.
    ///
    /// Lane identity derives from responsibility and config (see
    /// `cache_ids_for`); this field stays only because the non-owned
    /// plan caller still supplies it. Removal awaits that migration.
    pub(crate) lane: u32,
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
    /// Emitted report identities for named checks.
    pub(crate) named_check_lanes: &'a BTreeMap<String, Vec<NamedCheckLane>>,
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
    changed: Option<&BTreeSet<String>>,
    keys: &BTreeSet<String>,
) -> bool {
    changed.is_none_or(|set| group_changed(task, set, keys))
}

/// Extension bundle plus closure-bound identity digests for one task.
struct PlannedIdentity {
    /// Snapshot-indexed bundle with checkout-bound lock/Nextest digests.
    bundle: ExtensionBundle,
    /// Canonical digest over the complete input closure.
    closure_digest: String,
    /// Input digest with the closure digest bound into the envelope.
    input_digest: String,
}

/// Stack-extension envelope plus reuse eligibility for one task.
///
/// Closed per-stack dispatch: rust tasks derive through the rust
/// bridge over the snapshot bundle; tofu tasks derive through the
/// tofu bridge with a checkout-bound root-lockfile slot. Both feed
/// the same neutral envelope and gate.
fn extension_for_task(
    task: &ProposedTask,
    root: &Path,
    bundle: &ExtensionBundle,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<(StackExtension, bool), OrchestratorError> {
    let stack = Stack::require_known(&task.stack_id).map_err(internal_contract)?;
    if stack == Stack::Mise {
        return Err(internal("named_check_definition_required"));
    }
    if stack == Stack::Tofu {
        let ext = crate::internal_plan::tofu_extension_for(task, root, bundle, reads)
            .map_err(internal_contract)?;
        return Ok((ext.to_stack_extension(), ext.reuse_eligible().is_ok()));
    }
    let ext = extension_for_proposal(task, &bundle.inputs()).map_err(internal_contract)?;
    Ok((ext.to_stack_extension(), ext.reuse_eligible().is_ok()))
}

/// Snapshot bundle plus closure-bound identity digests for one task.
///
/// The closure resolves against the checkout and its digest binds into
/// the identity envelope, so a source edit flips `input_digest` even
/// when the changed-work hint misses it.
fn planned_identity(
    inputs: &GroupInputs<'_>,
    argv: &[String],
    toolchain: &str,
    platform_id: &str,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<PlannedIdentity, OrchestratorError> {
    let task = inputs.task;
    let manifest = task.identity.unit_path.clone();
    let nextest_config = nextest_config_for(inputs.discovery, task);
    let bundle = extension_bundle_with_snapshot(
        inputs.snapshot,
        inputs.discovery,
        task,
        Some(inputs.root),
        nextest_config.as_deref(),
    );
    let extension = extension_for_task(task, inputs.root, &bundle, reads)?.0;
    let closure = resolve_closure_at_root(
        inputs.root,
        task,
        nextest_config.as_deref(),
        bundle.graph_digest(),
        toolchain,
        platform_id,
        &mut *reads,
    )
    .map_err(internal_contract)?;
    let closure_digest = canonical_digest(&closure).map_err(internal_contract)?;
    let input_digest = task_identity_digest(&IdentityInputs {
        task,
        argv,
        toolchain_id: toolchain,
        platform_id,
        manifest: &manifest,
        generator: inputs.wire.generator,
        extension,
        closure_digest: &closure_digest,
    })
    .map_err(internal_contract)?;
    Ok(PlannedIdentity {
        bundle,
        closure_digest,
        input_digest,
    })
}

/// Obligation plus matrix entry for one universe member.
///
/// Identities attach first; changed members execute unconditionally
/// while unchanged members take the reuse outcome for later baseline
/// classification.
pub(crate) fn plan_group(
    inputs: &GroupInputs<'_>,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<(PlanObligation, Vec<MatrixEntry>), OrchestratorError> {
    let task = inputs.task;
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
    let argv = task_argv(task, inputs.catalog)?;
    let platform_id = platform_id_for_group(inputs.label, task).map_err(internal_contract)?;
    let identity = planned_identity(inputs, &argv, &toolchain, &platform_id, &mut *reads)?;
    let reuse_eligible = extension_for_task(task, inputs.root, &identity.bundle, reads)?.1;
    let input_digest = identity.input_digest;
    let closure_digest = identity.closure_digest;
    let reuse = if inputs.changed {
        wire_w2::ReuseOutcome::execute("affected_by_change")
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
    let gate = wire_w2::check_archive_identity_with_source(
        task,
        &toolchain,
        &platform_id,
        identity.bundle.config_digest(),
        Some(&closure_digest),
    )?;
    // The archive identity binds the content closure, never a manifest
    // pathname: a same-path source edit flips the closure and the
    // identity with it. Unbound sources refuse the task (execute with
    // reason); changed work already executes under its own reason.
    // Malformed specs stay hard errors: the planner generated them.
    let reuse = match gate {
        wire_w2::ArchiveGate::SourceUnbound if !inputs.changed => {
            wire_w2::ReuseOutcome::execute("archive_source_unbound")
        }
        _ => reuse,
    };
    complete_group(
        inputs,
        &argv,
        &toolchain,
        &input_digest,
        closure_digest,
        reuse,
    )
    .map(|(obligation, entry)| (obligation, vec![entry]))
}

/// Complete existing matrix transport after execution disposition is decided.
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
    // The persisted input digest flows through the validated reuse
    // outcome when one exists, so merge-time live comparison judges the
    // exact recorded value; forced-execution paths carry the identity.
    let recorded = reuse
        .recorded_input_digest
        .clone()
        .unwrap_or_else(|| input_digest.to_owned());
    let obligation = PlanObligation {
        task_id: task.task_id.clone(),
        decision: reuse.decision,
        reason: reuse.reason,
        task_digest: task_digest.clone(),
        input_digest: recorded,
        closure_digest,
        baseline_proof: None,
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
    let job_id = crate::crate_job_ids::job_id_for_member(&inputs.discovery.proposals, task)
        .ok_or_else(|| internal("crate_job_id_missing"))?;
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
    let cache_ids = cache_ids_for(task, inputs.label, toolchain).map_err(internal_contract)?;
    record_lane_target_dir(&mut entry.adapter_metadata, cache_ids.lane_id());
    entry.cache_ids = Some(cache_ids);
    Ok((obligation, entry))
}

/// Bind one task group's canonical plan identity to its selected label and target.
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
) -> Result<String, velnor_actions_contract::ContractError> {
    velnor_actions_contract::workflow::crate_job::task_digest_for_execution(
        task_id,
        argv,
        toolchain_id,
    )
}
