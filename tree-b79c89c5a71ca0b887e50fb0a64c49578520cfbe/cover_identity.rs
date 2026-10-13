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

use std::collections::BTreeSet;

use velnor_actions_contract::{
    BaselineProof, ObligationDecision, Plan, PlanObligation, digest_b3, validate_rust_extension,
};

use crate::cover_baseline::BaselineInputs;
use crate::cover_baseline::provenance_check::ValidatedProvenance;
use crate::discover::Discovery;
use crate::extension_schemas::coverage_schema_known;
use crate::external_data::{
    DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS, external_data_kind, may_skip_external_data,
};
use crate::internal::plan_obligation::{changed_keys, member_changed};
use crate::internal_plan::closure::resolve_closure_at_root;
use crate::internal_plan::{extension_bundle, nextest_config_for, toolchain_id};
use crate::merge::BaselineManifest;

pub(crate) use self::generator::{
    SOURCE_BUILD_REASON, is_source_build, resolve_generator_identity,
};

/// Refusal reason when the adapter extension forbids coverage, if any.
///
/// Groups missing from discovery refuse outright; undeclared reads and
/// conservative execution refuse as before; extension bytes must
/// validate (schema plus slots, not just the task-ID prefix); and the
/// input closure must resolve completely against the checkout, with
/// unknown inputs forbidding coverage.
fn coverage_refusal(
    discovery: &Discovery,
    task_id: &str,
    root: &std::path::Path,
    catalog: &velnor_actions_mise::ToolCatalog,
    label: &str,
) -> Option<String> {
    let group = discovery
        .task_groups
        .iter()
        .find(|group| group.task_id == task_id)?;
    if group.undeclared_reads {
        return Some("undeclared_inputs".to_owned());
    }
    let bundle = extension_bundle(discovery, group);
    let ext = group.identity_extension(&bundle.inputs());
    if ext.coverage_eligible().is_err() || ext.conservative_execution_required() {
        return Some("undeclared_inputs".to_owned());
    }
    if let Err(err) = validate_rust_extension(&ext.to_stack_extension()) {
        return Some(format!("extension_unverified:{err}"));
    }
    let Ok(toolchain) = toolchain_id(group, catalog) else {
        return Some("toolchain_unresolvable".to_owned());
    };
    let platform = digest_b3(label.as_bytes());
    let closure = resolve_closure_at_root(
        root,
        group,
        nextest_config_for(discovery, group).as_deref(),
        bundle.graph_digest(),
        &toolchain,
        &platform,
    );
    if closure.verify_complete().is_err() {
        return Some(format!(
            "incomplete_inputs:{}",
            closure.unknown_inputs().join(",")
        ));
    }
    None
}

/// Mark one obligation covered from validated provenance only.
fn mark_covered(
    obligation: &mut PlanObligation,
    task: &crate::merge::required_evidence::BaselineTaskEntry,
    provenance: &ValidatedProvenance,
) {
    obligation.decision = ObligationDecision::CoveredByTrustedBaseline;
    obligation.reason = String::from("covered_by_trusted_baseline");
    obligation.baseline_proof = Some(BaselineProof {
        source_commit: provenance.source_commit.clone(),
        run_id: task.proof_run_id,
        artifact_id: provenance.artifact_id,
        artifact_name: provenance.artifact_name.clone(),
        manifest_digest: provenance.manifest_digest.clone(),
    });
}

/// Mark covered obligations and prune the matrix; returns covered count.
///
/// Changed obligations never cover, even on identity match: the
/// changed hint guards identities that may miss semantic inputs.
/// Baseline provenance is set by the caller from the returned count.
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
        if member_changed(group, changed, &keys) {
            continue;
        }
        if let Some(reason) = coverage_refusal(
            discovery,
            &obligation.task_id,
            inputs.root,
            inputs.catalog,
            &plan.runner.label,
        ) {
            plan.warnings
                .push(format!("baseline_miss:{}:{reason}", obligation.task_id));
            continue;
        }
        if !coverage_schema_known(&obligation.task_id) {
            plan.warnings.push(format!(
                "baseline_miss:{}:unknown_extension_schema",
                obligation.task_id
            ));
            continue;
        }
        if external_data_kind(&obligation.task_id).is_some()
            && !may_skip_external_data(
                true,
                task.external_data.as_ref(),
                DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS,
            )
        {
            plan.warnings.push(format!(
                "baseline_miss:{}:external_data_rerun",
                obligation.task_id
            ));
            continue;
        }
        mark_covered(obligation, task, provenance);
        covered += 1;
    }
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
    covered
}
