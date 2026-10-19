//! Per-obligation identities and changed-work dispositions (P02).
//!
//! Every universe member gets content identities first; the changed-work
//! classification then assigns exactly one disposition: changed
//! obligations execute unconditionally, while unchanged ones take the
//! reuse outcome (execute until task-cache proof exists).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

use velnor_actions_contract::{
    MatrixEntry, NamedCheckLane, PlanObligation, ProposedTask, Stack, canonical_json_bytes,
    digest_b3,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::restore::probe_tool_availability;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::internal::{internal, internal_contract};
use crate::internal_plan::identities::{platform_id_for_group, toolchain_inputs_for};
use crate::internal_plan::snapshot::ExecutionSnapshot;
use crate::internal_plan::wire_w2::{self, GroupWire};
use crate::schedule::assign_lanes;
use crate::select::group_changed;
use crate::vectors::task_argv;

#[path = "plan_obligation_identity.rs"]
pub(crate) mod identity;
#[cfg(test)]
#[path = "plan_obligation_tests.rs"]
mod tests;

/// Inputs for planning one obligation.
pub(crate) struct GroupInputs<'a> {
    /// Validated discovery inventory.
    pub(crate) discovery: &'a Discovery,
    /// Complete job-owner index for this plan.
    pub(crate) job_owners: &'a crate::crate_job_ids::JobOwners<'a>,
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

type PlannedIdentity = identity::TaskIdentityDerivation;

#[path = "plan_obligation/complete.rs"]
mod complete;
#[cfg(test)]
use crate::internal_plan::execute_ids;
#[cfg(test)]
use complete::LaneEntryInputs;
#[cfg(test)]
use complete::derive_lane_entries;

#[cfg(test)]
use identity::extension_for_task;

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
    identity::derive_task_identity(
        &identity::TaskIdentityDerivationInputs {
            root: inputs.root,
            discovery: inputs.discovery,
            task: inputs.task,
            argv,
            toolchain,
            platform_id,
            generator: inputs.wire.generator,
            snapshot: inputs.snapshot,
        },
        reads,
    )
}

/// Obligation plus matrix entry for one universe member.
///
/// Identities attach first; changed members execute unconditionally
/// while unchanged members take the reuse outcome.
pub(crate) fn plan_group(
    inputs: &GroupInputs<'_>,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<(PlanObligation, Vec<MatrixEntry>), OrchestratorError> {
    let task = inputs.task;
    if Stack::from_id(&task.stack_id) == Some(Stack::Mise) {
        let item = crate::internal_plan::named_checks::discovered(inputs.discovery, task)
            .map_err(internal_contract)?;
        let job_id = if inputs.wire.event == velnor_actions_contract::WorkflowEvent::Qualification {
            crate::crate_job_ids::qualification_job_id_for_member(task)
                .map_err(internal_contract)?
        } else {
            format!("check-{}", item.check.id)
        };
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
    let toolchain_inputs = toolchain_inputs_for(task, inputs.catalog).map_err(internal_contract)?;
    let toolchain = velnor_actions_contract::cachekey::toolchain_id(&toolchain_inputs)
        .map_err(internal_contract)?;
    let argv = task_argv(task, inputs.catalog)?;
    let task_binding_digest =
        crate::tool_catalog_project::task_binding_digest(task, inputs.catalog)?;
    let platform_id = platform_id_for_group(inputs.label, task).map_err(internal_contract)?;
    let identity = planned_identity(inputs, &argv, &toolchain, &platform_id, &mut *reads)?;
    let reuse_eligible = identity.reuse_eligible;
    let input_digest = identity.input_digest;
    let closure_digest = identity.closure_digest;
    let graph_digest = identity.bundle.graph_digest().to_owned();
    let gate = wire_w2::check_archive_identity_with_source(
        task,
        &toolchain,
        &platform_id,
        identity.bundle.config_digest(),
        Some(&closure_digest),
        inputs.catalog,
    )?;
    // The archive identity binds the content closure, never a manifest
    // pathname: a same-path source edit flips the closure and the
    // identity with it. Unbound sources refuse the task (execute with
    // reason); changed work already executes under its own reason.
    // Malformed specs stay hard errors: the planner generated them.
    let reuse = if inputs.changed {
        wire_w2::ReuseOutcome::execute("affected_by_change")
    } else if gate == wire_w2::ArchiveGate::SourceUnbound {
        wire_w2::ReuseOutcome::execute("archive_source_unbound")
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
    complete::complete_group(
        inputs,
        complete::GroupCompletion {
            argv: &argv,
            task_binding_digest,
            toolchain: &toolchain,
            toolchain_inputs: &toolchain_inputs,
            input_digest: &input_digest,
            closure_digest,
            graph_digest,
            reuse,
        },
    )
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
