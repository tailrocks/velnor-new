//! Baseline coverage application over verified task identities.
//!
//! Coverage requires exact discovered identities and complete input closures.

// Wired here so generator resolution compiles without touching `lib.rs`.
pub mod generator;
// Unit tests live here so `cover_identity.rs` keeps its size gate.
#[cfg(test)]
mod tests;
// Structured-proof tests live apart for the same reason.
// Test fixtures live apart so the test module keeps its size gate.

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::{Stack, digest_b3};
use velnor_actions_contract_planning::ProposedTask;
use velnor_actions_contract_release::{validate_rust_extension, validate_tofu_extension};
use velnor_actions_contract_workflow::{
    BaselineProof, ManifestTaskProof, ObligationDecision, Plan, PlanObligation,
};
use velnor_actions_rust::{extension_for_proposal, tool_needs};

use crate::cover_baseline::BaselineInputs;
use crate::cover_baseline::provenance_check::ValidatedProvenance;
use velnor_actions_orchestrator_core::extension_schemas::coverage_schema_known;
use velnor_actions_orchestrator_discovery::discover::Discovery;
use velnor_actions_orchestrator_external_data::external_data::{
    DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS, external_data_kind, may_skip_external_data,
};
use velnor_actions_orchestrator_graph::internal_plan::closure::resolve_closure_at_root;
use velnor_actions_orchestrator_graph::internal_plan::identities::{
    extension_bundle_with_snapshot, platform_id_for_group,
};
use velnor_actions_orchestrator_graph::internal_plan::snapshot::{
    ExecutionSnapshot, canonical_digest,
};
use velnor_actions_orchestrator_graph::internal_plan::{nextest_config_for, toolchain_id};
use velnor_actions_orchestrator_merge_ports::{BaselineManifest, changed_keys, member_changed};

pub use self::generator::{SOURCE_BUILD_REASON, is_source_build};

fn cover_closure_digest(
    snapshot: &ExecutionSnapshot,
    discovery: &Discovery,
    task: &ProposedTask,
    root: &std::path::Path,
    catalog: &velnor_actions_mise::ToolCatalog,
    label: &str,
) -> Result<String, String> {
    if task.identity.undeclared_reads {
        return Err("undeclared_inputs".to_owned());
    }
    let bundle = extension_bundle_with_snapshot(
        snapshot,
        discovery,
        task,
        Some(root),
        nextest_config_for(discovery, task).as_deref(),
    );
    let mut reads = velnor_actions_tofu_core::FileCache::new();
    verify_cover_extension(task, root, &bundle, &mut reads)?;
    let Ok(toolchain) = toolchain_id(task, catalog) else {
        return Err("toolchain_unresolvable".to_owned());
    };
    let Ok(platform) = platform_id_for_group(label, task) else {
        return Err("platform_unresolvable".to_owned());
    };
    let Ok(closure) = resolve_closure_at_root(
        root,
        task,
        nextest_config_for(discovery, task).as_deref(),
        bundle.graph_digest(),
        &toolchain,
        &platform,
        &mut reads,
    ) else {
        return Err("closure_unresolvable".to_owned());
    };
    if closure.verify_complete().is_err() {
        return Err(format!(
            "incomplete_inputs:{}",
            closure.unknown_inputs().join(",")
        ));
    }
    canonical_digest(&closure).map_err(|_| "closure_digest_failed".to_owned())
}

fn verify_cover_extension(
    task: &ProposedTask,
    root: &Path,
    bundle: &velnor_actions_orchestrator_graph::internal_plan::identities::ExtensionBundle,
    reads: &mut velnor_actions_tofu_core::FileCache,
) -> Result<(), String> {
    let stack =
        Stack::require_known(&task.stack_id).map_err(|e| format!("extension_unverified:{e}"))?;
    if stack == Stack::Mise {
        return Err("undeclared_inputs".to_owned());
    }
    if stack == Stack::Tofu {
        let ext = velnor_actions_orchestrator_graph::internal_plan::tofu_extension_for(
            task, root, bundle, reads,
        )
        .map_err(|_| "extension_unverified:unparsable_spelling".to_owned())?;
        if ext.coverage_eligible().is_err() || ext.conservative_execution_required() {
            return Err("undeclared_inputs".to_owned());
        }
        return validate_tofu_extension(&ext.to_stack_extension())
            .map_err(|err| format!("extension_unverified:{err}"));
    }
    let ext = extension_for_proposal(task, &bundle.inputs())
        .map_err(|_| "extension_unverified:unparsable_spelling".to_owned())?;
    if ext.coverage_eligible().is_err() || ext.conservative_execution_required() {
        return Err("undeclared_inputs".to_owned());
    }
    validate_rust_extension(&ext.to_stack_extension())
        .map_err(|err| format!("extension_unverified:{err}"))
}

fn live_mbx_digest(task: &ProposedTask, catalog: &velnor_actions_mise::ToolCatalog) -> String {
    let mbx = if Stack::from_id(&task.stack_id) == Some(Stack::Rust) {
        tool_needs(&task.identity.compile_driver, &task.identity.test_runner).mbx
    } else {
        false
    };
    let pin = mbx.then(|| catalog.version(velnor_actions_mise::PinnedTool::MrBoxington));
    canonical_digest(&serde_json::json!({
        "driver": task.identity.compile_driver,
        "mbx_pin": pin,
    }))
    .unwrap_or_else(|_| digest_b3(b"mbx_error"))
}

fn verify_proof_live(
    snapshot: &ExecutionSnapshot,
    discovery: &Discovery,
    task: &ProposedTask,
    proof: &ManifestTaskProof,
    root: &Path,
    catalog: &velnor_actions_mise::ToolCatalog,
    label: &str,
) -> Result<(), String> {
    let bundle = extension_bundle_with_snapshot(
        snapshot,
        discovery,
        task,
        Some(root),
        nextest_config_for(discovery, task).as_deref(),
    );
    if proof.graph_digest() != bundle.graph_digest() {
        return Err("proof_graph_mismatch".to_owned());
    }
    let toolchain = toolchain_id(task, catalog).map_err(|_| "toolchain_unresolvable".to_owned())?;
    if proof.toolchain_id() != toolchain {
        return Err("proof_toolchain_mismatch".to_owned());
    }
    let Ok(platform) = platform_id_for_group(label, task) else {
        return Err("platform_unresolvable".to_owned());
    };
    if proof.platform_id() != platform {
        return Err("proof_platform_mismatch".to_owned());
    }
    if proof.profile() != bundle.inputs().profile {
        return Err("proof_profile_mismatch".to_owned());
    }
    if proof.mbx_digest() != live_mbx_digest(task, catalog) {
        return Err("proof_mbx_mismatch".to_owned());
    }
    Ok(())
}

/// the recorded entry digest; any drift executes instead of covering.
fn verified_closure_digest(
    snapshot: &ExecutionSnapshot,
    discovery: &Discovery,
    task: &ProposedTask,
    entry_digest: &str,
    root: &std::path::Path,
    catalog: &velnor_actions_mise::ToolCatalog,
    label: &str,
) -> Result<String, String> {
    let live = cover_closure_digest(snapshot, discovery, task, root, catalog, label)?;
    if entry_digest == live {
        Ok(live)
    } else {
        Err("closure_mismatch".to_owned())
    }
}

/// Mark one obligation covered from validated provenance only.
///
fn mark_covered(
    obligation: &mut PlanObligation,
    task: &velnor_actions_orchestrator_merge::required_evidence::BaselineTaskEntry,
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
/// A carried proof compares all five dimensions against live identity;
/// drift refuses with its miss warning, and a match passes silently.
fn gate_proof(
    warnings: &mut Vec<String>,
    label: &str,
    task: &velnor_actions_orchestrator_merge::required_evidence::BaselineTaskEntry,
    proposal: &ProposedTask,
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
        proposal,
        proof,
        inputs.root,
        inputs.catalog,
        label,
    ) {
        warnings.push(format!("baseline_miss:{}:{reason}", proposal.task_id));
        return false;
    }
    true
}

/// Schema and external-data gates for one candidate entry.
///
/// Unknown extension schemas never cover, and advisory external data
/// forces a rerun unless fresh enough to skip.
fn gate_guards(
    warnings: &mut Vec<String>,
    task_id: &str,
    task: &velnor_actions_orchestrator_merge::required_evidence::BaselineTaskEntry,
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

/// Opaque adapter tasks never qualify for baseline coverage.
fn gate_opaque(warnings: &mut Vec<String>, task: &ProposedTask) -> bool {
    if task.identity.undeclared_reads || Stack::from_id(&task.stack_id) == Some(Stack::Mise) {
        warnings.push(format!("baseline_miss:{}:undeclared_inputs", task.task_id));
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
pub fn apply_coverage(
    plan: &mut Plan,
    manifest: &BaselineManifest,
    provenance: &ValidatedProvenance,
    discovery: &Discovery,
    changed: Option<&BTreeSet<String>>,
    inputs: &BaselineInputs<'_>,
) -> u32 {
    let keys = changed
        .map(|set| changed_keys(&discovery.proposals.iter().collect::<Vec<_>>(), set))
        .unwrap_or_default();
    let snapshot = ExecutionSnapshot::build(discovery);
    let mut covered = 0u32;
    for obligation in &mut plan.obligations {
        let proposal = discovery
            .proposals
            .iter()
            .find(|proposal| proposal.task_id == obligation.task_id);
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
        let Some(proposal) = proposal else {
            plan.warnings.push(format!(
                "baseline_miss:{}:undiscovered_task_group",
                obligation.task_id
            ));
            continue;
        };
        if !gate_opaque(&mut plan.warnings, proposal) {
            continue;
        }
        if !gate_proof(
            &mut plan.warnings,
            &plan.runner.label,
            task,
            proposal,
            &snapshot,
            discovery,
            inputs,
        ) {
            continue;
        }
        if member_changed(proposal, changed, &keys) {
            continue;
        }
        if let Err(reason) = verified_closure_digest(
            &snapshot,
            discovery,
            proposal,
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
