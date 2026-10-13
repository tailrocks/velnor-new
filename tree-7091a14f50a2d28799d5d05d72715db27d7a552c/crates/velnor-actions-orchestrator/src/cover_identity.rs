//! Baseline coverage application over verified task identities.
//!
//! Coverage grants only when the obligation's digests match, the task
//! group is discovered, its extension bytes validate, and its input
//! closure resolves completely against the checkout.

// Wired here so generator resolution compiles without touching `lib.rs`.
#[path = "generator.rs"]
pub(crate) mod generator;
// Unit tests live here so `cover_identity.rs` keeps its size gate.
#[cfg(test)]
#[path = "cover_identity_tests.rs"]
mod cover_identity_tests;
// Test fixtures live apart so the test module keeps its size gate.
#[cfg(test)]
#[path = "cover_identity_fixtures.rs"]
mod cover_identity_fixtures;

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::{
    BaselineProof, ManifestTaskProof, ObligationDecision, Plan, PlanObligation,
    validate_rust_extension,
};
use velnor_actions_rust::TaskGroup;

use crate::cover_baseline::BaselineInputs;
use crate::cover_baseline::provenance_check::ValidatedProvenance;
use crate::discover::Discovery;
use crate::extension_schemas::coverage_schema_known;
use crate::external_data::{
    DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS, external_data_kind, may_skip_external_data,
};
use crate::internal::plan_obligation::{changed_keys, member_changed};
use crate::internal_plan::closure::resolve_closure_at_root;
use crate::internal_plan::identities::{extension_bundle_with_snapshot, platform_id_for_group};
use crate::internal_plan::snapshot::{ExecutionSnapshot, canonical_digest};
use crate::internal_plan::{nextest_config_for, toolchain_id};
use crate::merge::BaselineManifest;

pub(crate) use self::generator::{SOURCE_BUILD_REASON, is_source_build};

/// Explicit unpassed acceptance: a structured task proof carries `mbx`
/// and `profile` dimensions for which the data model defines no live
/// source (no publisher binds them), so coverage compares the
/// graph/toolchain/platform dimensions and records these two openly
/// instead of passing silently.
pub(crate) const PROOF_DIM_COMPARISON_GAP: &str = "unpassed:proof_mbx_profile_have_no_live_source";

/// Cover-time closure digest for one group, or the refusal reason.
///
/// Undeclared reads and conservative execution refuse as before;
/// extension bytes must validate (schema plus slots, not just the
/// task-ID prefix); and the input closure must resolve completely
/// against the checkout, with unknown inputs forbidding coverage.
/// Graph digests come from the once-built snapshot index, never a
/// per-group rescan. The caller compares the returned live digest
/// against the baseline entry: only an exact match covers, so a source
/// edit refuses even when the changed-work hint misses it.
fn cover_closure_digest(
    snapshot: &ExecutionSnapshot,
    discovery: &Discovery,
    group: &velnor_actions_rust::TaskGroup,
    root: &std::path::Path,
    catalog: &velnor_actions_mise::ToolCatalog,
    label: &str,
) -> Result<String, String> {
    if group.undeclared_reads {
        return Err("undeclared_inputs".to_owned());
    }
    let bundle = extension_bundle_with_snapshot(
        snapshot,
        discovery,
        group,
        Some(root),
        nextest_config_for(discovery, group).as_deref(),
    );
    let ext = group.identity_extension(&bundle.inputs());
    if ext.coverage_eligible().is_err() || ext.conservative_execution_required() {
        return Err("undeclared_inputs".to_owned());
    }
    if let Err(err) = validate_rust_extension(&ext.to_stack_extension()) {
        return Err(format!("extension_unverified:{err}"));
    }
    let Ok(toolchain) = toolchain_id(group, catalog) else {
        return Err("toolchain_unresolvable".to_owned());
    };
    let platform = platform_id_for_group(label, group);
    let closure = resolve_closure_at_root(
        root,
        group,
        nextest_config_for(discovery, group).as_deref(),
        bundle.graph_digest(),
        &toolchain,
        &platform,
    );
    if closure.verify_complete().is_err() {
        return Err(format!(
            "incomplete_inputs:{}",
            closure.unknown_inputs().join(",")
        ));
    }
    canonical_digest(&closure).map_err(|_| "closure_digest_failed".to_owned())
}

/// Structured proof verified against live task identity.
///
/// Compares the carried graph, toolchain, and platform dimensions
/// against the values resolved live for this group; any drift refuses
/// coverage. The `mbx` and `profile` dimensions have no defined live
/// source (see [`PROOF_DIM_COMPARISON_GAP`]) and stay uncompared but
/// explicitly recorded by the caller.
fn verify_proof_live(
    snapshot: &ExecutionSnapshot,
    discovery: &Discovery,
    group: &TaskGroup,
    proof: &ManifestTaskProof,
    root: &Path,
    catalog: &velnor_actions_mise::ToolCatalog,
    label: &str,
) -> Result<(), String> {
    let bundle = extension_bundle_with_snapshot(
        snapshot,
        discovery,
        group,
        Some(root),
        nextest_config_for(discovery, group).as_deref(),
    );
    if proof.graph_digest() != bundle.graph_digest() {
        return Err("proof_graph_mismatch".to_owned());
    }
    let toolchain =
        toolchain_id(group, catalog).map_err(|_| "toolchain_unresolvable".to_owned())?;
    if proof.toolchain_id() != toolchain {
        return Err("proof_toolchain_mismatch".to_owned());
    }
    if proof.platform_id() != platform_id_for_group(label, group) {
        return Err("proof_platform_mismatch".to_owned());
    }
    Ok(())
}

/// Live closure digest verified against the baseline entry.
///
/// Resolves the cover-time closure (refusing unknown inputs, undeclared
/// reads, and unverified extensions) and requires an exact match with
/// the recorded entry digest; any drift executes instead of covering.
fn verified_closure_digest(
    snapshot: &ExecutionSnapshot,
    discovery: &Discovery,
    group: &velnor_actions_rust::TaskGroup,
    entry_digest: &str,
    root: &std::path::Path,
    catalog: &velnor_actions_mise::ToolCatalog,
    label: &str,
) -> Result<String, String> {
    let live = cover_closure_digest(snapshot, discovery, group, root, catalog, label)?;
    if entry_digest == live {
        Ok(live)
    } else {
        Err("closure_mismatch".to_owned())
    }
}

/// Mark one obligation covered from validated provenance only.
///
/// Returns false when the proof constructor rejects its inputs; the
/// caller keeps the obligation executing instead of storing a forged
/// or partial proof.
fn mark_covered(
    obligation: &mut PlanObligation,
    task: &crate::merge::required_evidence::BaselineTaskEntry,
    provenance: &ValidatedProvenance,
) -> bool {
    let Ok(proof) = BaselineProof::new(
        &provenance.source_commit,
        task.proof_run_id,
        provenance.artifact_id,
        &provenance.artifact_name,
        &provenance.manifest_digest,
    ) else {
        return false;
    };
    obligation.decision = ObligationDecision::CoveredByTrustedBaseline;
    obligation.reason = String::from("covered_by_trusted_baseline");
    obligation.baseline_proof = Some(proof);
    true
}

/// Structured proof gate for one candidate entry.
///
/// A carried proof compares against live identity; drift refuses with
/// its miss warning, and a match records the mbx/profile comparison
/// gap openly. Entries without a proof pass through untouched.
fn gate_proof(
    warnings: &mut Vec<String>,
    label: &str,
    task: &crate::merge::required_evidence::BaselineTaskEntry,
    group: &TaskGroup,
    snapshot: &ExecutionSnapshot,
    discovery: &Discovery,
    inputs: &BaselineInputs<'_>,
) -> bool {
    let Some(proof) = &task.proof else {
        return true;
    };
    if let Err(reason) = verify_proof_live(
        snapshot,
        discovery,
        group,
        proof,
        inputs.root,
        inputs.catalog,
        label,
    ) {
        warnings.push(format!("baseline_miss:{}:{reason}", group.task_id));
        return false;
    }
    warnings.push(format!(
        "baseline_note:{}:{PROOF_DIM_COMPARISON_GAP}",
        group.task_id
    ));
    true
}

/// Schema and external-data gates for one candidate entry.
///
/// Unknown extension schemas never cover, and advisory external data
/// forces a rerun unless fresh enough to skip.
fn gate_guards(
    warnings: &mut Vec<String>,
    task_id: &str,
    task: &crate::merge::required_evidence::BaselineTaskEntry,
) -> bool {
    if !coverage_schema_known(task_id) {
        warnings.push(format!("baseline_miss:{task_id}:unknown_extension_schema"));
        return false;
    }
    if external_data_kind(task_id).is_some()
        && !may_skip_external_data(
            true,
            task.external_data.as_ref(),
            DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS,
        )
    {
        warnings.push(format!("baseline_miss:{task_id}:external_data_rerun"));
        return false;
    }
    true
}

/// Mark covered obligations and prune the matrix; returns covered count.
///
/// Coverage needs an exact closure-digest match against the checkout:
/// the changed hint stays as defense-in-depth only and never decides
/// soundness alone. Baseline provenance is set by the caller from the
/// returned count.
pub(crate) fn apply_coverage(
    plan: &mut Plan,
    manifest: &BaselineManifest,
    provenance: &ValidatedProvenance,
    discovery: &Discovery,
    changed: Option<&BTreeSet<String>>,
    inputs: &BaselineInputs<'_>,
) -> u32 {
    let universe: Vec<_> = discovery.task_groups.iter().collect();
    let keys = changed
        .map(|set| changed_keys(&universe, set))
        .unwrap_or_default();
    let snapshot = ExecutionSnapshot::build(discovery);
    let mut covered = 0u32;
    for obligation in &mut plan.obligations {
        let group = discovery
            .task_groups
            .iter()
            .find(|group| group.task_id == obligation.task_id);
        let hit = manifest.tasks.iter().find(|task| {
            task.task_id == obligation.task_id
                && task.task_digest == obligation.task_digest
                && task.input_digest == obligation.input_digest
        });
        let Some(task) = hit else {
            plan.warnings
                .push(format!("baseline_miss:{}:no_entry", obligation.task_id));
            continue;
        };
        let Some(group) = group else {
            plan.warnings.push(format!(
                "baseline_miss:{}:undiscovered_task_group",
                obligation.task_id
            ));
            continue;
        };
        if !gate_proof(
            &mut plan.warnings,
            &plan.runner.label,
            task,
            group,
            &snapshot,
            discovery,
            inputs,
        ) {
            continue;
        }
        if member_changed(group, changed, &keys) {
            continue;
        }
        if let Err(reason) = verified_closure_digest(
            &snapshot,
            discovery,
            group,
            &task.closure_digest,
            inputs.root,
            inputs.catalog,
            &plan.runner.label,
        ) {
            plan.warnings
                .push(format!("baseline_miss:{}:{reason}", obligation.task_id));
            continue;
        }
        if !gate_guards(&mut plan.warnings, &obligation.task_id, task) {
            continue;
        }
        if mark_covered(obligation, task, provenance) {
            covered += 1;
        } else {
            plan.warnings.push(format!(
                "baseline_miss:{}:proof_unconstructible",
                obligation.task_id
            ));
        }
    }
    prune_to_execute(plan);
    covered
}

/// Drop covered matrix entries and deselect fully-covered packages.
fn prune_to_execute(plan: &mut Plan) {
    plan.matrix.include.retain(|entry| {
        plan.obligations
            .iter()
            .any(|ob| ob.task_id == entry.task_id && ob.decision == ObligationDecision::Execute)
    });
    for package in &mut plan.packages {
        package.selected = plan.obligations.iter().any(|ob| {
            ob.decision == ObligationDecision::Execute && package.tasks.contains(&ob.task_id)
        });
    }
}
