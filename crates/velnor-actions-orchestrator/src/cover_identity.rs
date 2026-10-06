//! Baseline coverage application over verified task identities.
//!
//! Coverage grants only when the obligation's digests match, the task
//! proposal is discovered, its extension bytes validate, and its input
//! closure resolves completely against the checkout.

// Wired here so generator resolution compiles without touching `lib.rs`.
#[path = "generator.rs"]
pub(crate) mod generator;
// Unit tests live here so `cover_identity.rs` keeps its size gate.
#[cfg(test)]
#[path = "cover_identity_tests.rs"]
mod cover_identity_tests;
// Structured-proof tests live apart for the same reason.
#[cfg(test)]
#[path = "cover_missing_proof_tests.rs"]
mod cover_missing_proof_tests;
#[cfg(test)]
#[path = "cover_proof_tests.rs"]
mod cover_proof_tests;
// Test fixtures live apart so the test module keeps its size gate.
#[cfg(test)]
#[path = "cover_identity_fixtures.rs"]
pub(crate) mod cover_identity_fixtures;

use std::path::Path;

use velnor_actions_contract::{
    BaselineProof, ManifestTaskProof, ObligationDecision, Plan, PlanObligation, ProposedTask,
    Stack, validate_rust_extension, validate_tofu_extension,
};
use velnor_actions_rust::extension_for_proposal;

use crate::cover_baseline::BaselineInputs;
use crate::cover_baseline::provenance_check::ValidatedProvenance;
use crate::discover::Discovery;
use crate::extension_schemas::coverage_schema_known;
use crate::external_data::{
    DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS, external_data_kind, may_skip_external_data,
};
use crate::internal::plan_obligation::{changed_keys, member_changed};
use crate::internal_plan::closure::resolve_closure_at_root;

#[path = "cover_refinement.rs"]
mod refinement;

#[path = "cover_prune.rs"]
mod prune;
use crate::internal_plan::identities::{
    extension_bundle_with_snapshot, live_mbx_digest, platform_id_for_group,
};
use crate::internal_plan::snapshot::{ExecutionSnapshot, canonical_digest};
use crate::internal_plan::{nextest_config_for, toolchain_id_for_runner};
use crate::merge::BaselineManifest;

pub(crate) use self::generator::{SOURCE_BUILD_REASON, is_source_build};

/// Cover-time closure digest for one task, or the refusal reason.
///
/// Undeclared reads and conservative execution refuse as before;
/// extension bytes must validate (schema plus slots, not just the
/// task-ID prefix); and the input closure must resolve completely
/// against the checkout, with unknown inputs forbidding coverage.
/// Graph digests come from the once-built snapshot index, never a
/// per-task rescan. The caller compares the returned live digest
/// against the baseline entry: only an exact match covers, so a source
/// edit refuses even when the changed-work hint misses it.
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
    let mut reads = velnor_actions_tofu::FileCache::new();
    verify_cover_extension(task, root, &bundle, &mut reads)?;
    let Ok(toolchain) = toolchain_id_for_runner(task, catalog, label) else {
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
        Some(snapshot.checkout_inputs()),
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

/// Cover-time extension revalidation, dispatched per stack.
///
/// Rust tasks revalidate through the rust bridge; tofu tasks through
/// the shared tofu constructor plus the tofu slot validator. Spelling
/// drift or undeclared/unknown inputs refuse coverage either way.
fn verify_cover_extension(
    task: &ProposedTask,
    root: &Path,
    bundle: &crate::internal_plan::identities::ExtensionBundle,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<(), String> {
    if Stack::from_id(&task.stack_id) == Some(Stack::Workload) {
        return Err("undeclared_inputs".to_owned());
    }
    if Stack::from_id(&task.stack_id) == Some(Stack::Tofu) {
        let ext = crate::internal_plan::tofu_extension_for(task, root, bundle, reads)
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

/// Structured proof verified against live task identity.
///
/// Compares all five carried dimensions against the values resolved
/// live for this task: graph, toolchain, platform, execution profile,
/// and mbx. Any drift refuses coverage; nothing compares-and-notes.
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
    let toolchain = toolchain_id_for_runner(task, catalog, label)
        .map_err(|_| "toolchain_unresolvable".to_owned())?;
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

/// Live closure digest verified against the baseline entry.
///
/// Resolves the cover-time closure (refusing unknown inputs, undeclared
/// reads, and unverified extensions) and requires an exact match with
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
/// A carried proof compares all five dimensions against live identity;
/// drift refuses with its miss warning, and a match passes silently.
/// Missing proof refuses coverage, even when opaque digests match.
fn gate_proof(
    warnings: &mut Vec<String>,
    label: &str,
    candidate: (
        &crate::merge::required_evidence::BaselineTaskEntry,
        &PlanObligation,
    ),
    proposal: &ProposedTask,
    snapshot: &ExecutionSnapshot,
    discovery: &Discovery,
    inputs: &BaselineInputs<'_>,
) -> bool {
    let (task, obligation) = candidate;
    let Some(proof) = &task.proof else {
        warnings.push(format!(
            "baseline_miss:{}:missing_task_proof",
            proposal.task_id
        ));
        return false;
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
    if !obligation.execution_identity.matches_proof(proof) {
        warnings.push(format!(
            "baseline_miss:{}:proof_plan_identity_mismatch",
            proposal.task_id
        ));
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
    changed: Option<&crate::select::ChangedSelection>,
    inputs: &BaselineInputs<'_>,
) -> u32 {
    let universe: Vec<_> = discovery.proposals.iter().collect();
    let keys = changed
        .map(|set| changed_keys(&universe, &set.affected))
        .unwrap_or_default();
    let snapshot = ExecutionSnapshot::build(discovery).with_checkout(inputs.root);
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
        if !gate_proof(
            &mut plan.warnings,
            &plan.runner.label,
            (task, obligation),
            proposal,
            &snapshot,
            discovery,
            inputs,
        ) {
            continue;
        }
        if member_changed(proposal, changed, &keys)
            && !refinement::permits(proposal, changed, &universe, task)
        {
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
    prune::prune_to_execute(plan);
    covered
}
