//! Per-obligation identities and changed-work dispositions (P02).
//!
//! Every universe member gets content identities first; the changed-work
//! classification then assigns exactly one disposition: changed
//! obligations execute unconditionally, while unchanged ones take the
//! reuse outcome (execute until task-cache proof exists) and stay
//! eligible for later baseline coverage.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use velnor_actions_contract::{MatrixEntry, PlanObligation, canonical_json_bytes, digest_b3};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::restore::probe_tool_availability;
use velnor_actions_rust::TaskGroup;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::internal::{internal, internal_contract};
use crate::internal_plan::closure::resolve_closure_at_root;
use crate::internal_plan::identities::{
    ExtensionBundle, extension_bundle_with_snapshot, platform_id_for_group,
};
use crate::internal_plan::snapshot::{ExecutionSnapshot, canonical_digest};
use crate::internal_plan::wire_w2::{self, GroupWire};
use crate::internal_plan::{
    IdentityInputs, adapter_metadata, cache_ids_for, evidence_for_group, execute_ids,
    manifest_for_key, nextest_config_for, record_task_cache, task_identity_digest, toolchain_id,
};
use crate::schedule::assign_lanes;
use crate::select::group_changed;
use crate::vectors::task_argv;

/// Inputs for planning one obligation.
pub(crate) struct GroupInputs<'a> {
    /// Validated discovery inventory.
    pub(crate) discovery: &'a Discovery,
    /// Universe member to plan.
    pub(crate) group: &'a TaskGroup,
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
}

/// Deterministic lane per universe task ID.
pub(crate) fn lane_table(universe: &[&TaskGroup]) -> BTreeMap<String, u32> {
    let ids: Vec<String> = universe.iter().map(|group| group.task_id.clone()).collect();
    assign_lanes(&ids).into_iter().collect()
}

/// Changed manifest keys for groups with empty package IDs.
pub(crate) fn changed_keys(
    universe: &[&TaskGroup],
    changed: &BTreeSet<String>,
) -> BTreeSet<String> {
    universe
        .iter()
        .filter(|group| changed.contains(&group.package_id))
        .map(|group| group.manifest_key.clone())
        .collect()
}

/// True when one universe member counts as changed.
pub(crate) fn member_changed(
    group: &TaskGroup,
    changed: Option<&BTreeSet<String>>,
    keys: &BTreeSet<String>,
) -> bool {
    changed.is_none_or(|set| group_changed(group, set, keys))
}

/// Extension bundle plus closure-bound identity digests for one group.
struct PlannedIdentity {
    /// Snapshot-indexed bundle with checkout-bound lock/Nextest digests.
    bundle: ExtensionBundle,
    /// Canonical digest over the complete input closure.
    closure_digest: String,
    /// Input digest with the closure digest bound into the envelope.
    input_digest: String,
}

/// Snapshot bundle plus closure-bound identity digests for one group.
///
/// The closure resolves against the checkout and its digest binds into
/// the identity envelope, so a source edit flips `input_digest` even
/// when the changed-work hint misses it.
fn planned_identity(
    inputs: &GroupInputs<'_>,
    argv: &[String],
    toolchain: &str,
    platform_id: &str,
) -> Result<PlannedIdentity, OrchestratorError> {
    let group = inputs.group;
    let manifest = manifest_for_key(&group.manifest_key);
    let nextest_config = nextest_config_for(inputs.discovery, group);
    let bundle = extension_bundle_with_snapshot(
        inputs.snapshot,
        inputs.discovery,
        group,
        Some(inputs.root),
        nextest_config.as_deref(),
    );
    let ext = group.identity_extension(&bundle.inputs());
    let closure = resolve_closure_at_root(
        inputs.root,
        group,
        nextest_config.as_deref(),
        bundle.graph_digest(),
        toolchain,
        platform_id,
    );
    let closure_digest = canonical_digest(&closure).map_err(internal_contract)?;
    let input_digest = task_identity_digest(&IdentityInputs {
        group,
        argv,
        toolchain_id: toolchain,
        platform_id,
        manifest: &manifest,
        generator: inputs.wire.generator,
        extension: ext.to_stack_extension(),
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
) -> Result<(PlanObligation, MatrixEntry), OrchestratorError> {
    let group = inputs.group;
    let _ = inputs.lane;
    let toolchain = toolchain_id(group, inputs.catalog).map_err(internal_contract)?;
    let argv = task_argv(group, inputs.catalog)?;
    let platform_id = platform_id_for_group(inputs.label, group);
    let identity = planned_identity(inputs, &argv, &toolchain, &platform_id)?;
    let ext = group.identity_extension(&identity.bundle.inputs());
    let input_digest = identity.input_digest;
    let closure_digest = identity.closure_digest;
    let reuse = if inputs.changed {
        wire_w2::ReuseOutcome::execute("affected_by_change")
    } else {
        wire_w2::plan_reuse_outcome(
            group,
            inputs.wire.event,
            probe_tool_availability(false, false),
            &toolchain,
            &input_digest,
            ext.reuse_eligible().is_ok(),
        )?
    };
    let gate = wire_w2::check_archive_identity_with_source(
        group,
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
    let task_digest = task_digest(&group.task_id, &argv, &toolchain).map_err(internal_contract)?;
    let obligation = PlanObligation {
        task_id: group.task_id.clone(),
        decision: reuse.decision,
        reason: reuse.reason,
        task_digest: task_digest.clone(),
        input_digest: input_digest.clone(),
        closure_digest,
        baseline_proof: None,
    };
    let mut metadata = adapter_metadata(group, evidence_for_group(inputs.discovery, group));
    record_task_cache(
        &mut metadata,
        reuse.task_cache_enabled,
        reuse.task_cache_key.as_deref(),
    );
    let run = velnor_actions_workflow_renderer::join_argv_for_run(&argv)
        .map_err(|err| internal(&err.to_string()))?;
    let mut entry = MatrixEntry::derive(
        velnor_actions_rust::STACK_ID,
        &group.task_id,
        &run,
        &task_digest,
        metadata,
        execute_ids(group),
        &input_digest,
        inputs.run_key,
    )
    .map_err(internal_contract)?;
    let cache_ids = cache_ids_for(group, inputs.label, &toolchain).map_err(internal_contract)?;
    record_lane_target_dir(&mut entry.adapter_metadata, cache_ids.lane_id());
    entry.cache_ids = Some(cache_ids);
    Ok((obligation, entry))
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

/// Digest of canonical bytes for a serializable input struct.
fn digest_of<T: Serialize>(inputs: &T) -> Result<String, velnor_actions_contract::ContractError> {
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
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
    digest_of(&TaskDigestInputs {
        task_id,
        argv,
        toolchain_id,
    })
}

/// Task-digest preimage fields.
#[derive(Debug, Serialize)]
struct TaskDigestInputs<'a> {
    /// Stable task ID.
    task_id: &'a str,
    /// Fixed argument vector.
    argv: &'a [String],
    /// Toolchain identity digest.
    toolchain_id: &'a str,
}
