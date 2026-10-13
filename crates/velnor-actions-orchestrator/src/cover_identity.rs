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

use std::collections::BTreeSet;

use velnor_actions_contract::{BaselineProof, ObligationDecision, Plan};
use velnor_actions_mise::catalog::lock::{load_text, parse_generator_lock};

use crate::cover_baseline::BaselineInputs;
use crate::cover_baseline::provenance_check::ValidatedProvenance;
use crate::discover::Discovery;
use crate::extension_schemas::coverage_schema_known;
use crate::external_data::{
    DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS, external_data_kind, may_skip_external_data,
};
use crate::internal::plan_obligation::{changed_keys, member_changed};
use crate::internal_plan::extension_bundle;
use crate::merge::BaselineManifest;

pub(crate) use self::generator::{
    SOURCE_BUILD_REASON, is_source_build, resolve_generator_identity,
};

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

/// Mark covered obligations and prune the matrix; returns covered count.
///
/// Source builds record `{arch}-{os}`; release locks pin full triples.
fn release_target(target: &str) -> Option<&str> {
    if velnor_actions_contract::SUPPORTED_TARGETS.contains(&target) {
        return Some(target);
    }
    match target {
        "x86_64-linux" => Some("x86_64-unknown-linux-gnu"),
        "aarch64-macos" => Some("aarch64-apple-darwin"),
        "x86_64-macos" => Some("x86_64-apple-darwin"),
        _ => None,
    }
}

/// Mark covered obligations and prune the matrix; returns covered count.
///
/// Changed obligations never cover, even on identity match: the
/// changed hint guards identities that may miss semantic inputs.
/// Baseline provenance is set by the caller from the returned count.
pub(crate) fn apply_coverage(
    plan: &mut Plan,
    manifest: &BaselineManifest,
    digest: &str,
    discovery: &Discovery,
    changed: Option<&BTreeSet<String>>,
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
        if group.is_none_or(|group| member_changed(group, changed, &keys)) {
            continue;
        }
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
        obligation.decision = ObligationDecision::CoveredByTrustedBaseline;
        obligation.reason = String::from("covered_by_trusted_baseline");
        obligation.baseline_proof = Some(BaselineProof {
            source_commit: manifest.source_commit.clone(),
            run_id: task.proof_run_id,
            artifact_id: manifest.artifact_id,
            artifact_name: manifest.artifact_name.clone(),
            manifest_digest: digest.to_owned(),
        });
        covered += 1;
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
    covered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_data::ExternalDataFreshness;
    use crate::merge::BaselineManifest;
    use crate::merge::required_evidence::BaselineTaskEntry;
    use velnor_actions_contract::{
        BaselineStatus, ObligationDecision, PlanBaseline, PlanGenerator, PlanMatrix,
        PlanObligation, PlanRunner, RunnerSelection, Trust, WorkflowEvent, digest_b3,
    };

    /// Plan carrying one execute obligation per `task_ids`.
    fn plan_with(task_ids: &[&str]) -> Plan {
        let digest = digest_b3(b"digest");
        Plan {
            schema: 1,
            run_key: "local".to_owned(),
            plan_id: "plan-local".to_owned(),
            base: None,
            head: "head".to_owned(),
            event: WorkflowEvent::PullRequest,
            runner: PlanRunner {
                label: "ubuntu-26.04".to_owned(),
                selection: RunnerSelection::LatestDefault,
            },
            trust: Trust::Pr,
            baseline: PlanBaseline {
                status: BaselineStatus::Unavailable,
                base_commit: None,
                run_id: None,
                artifact_id: None,
                artifact_name: None,
                manifest_digest: None,
                reason: None,
            },
            generator: PlanGenerator {
                version: "0.1.0".to_owned(),
                target: "x86_64-linux".to_owned(),
                sha256: "0".repeat(64),
            },
            packages: Vec::new(),
            obligations: task_ids
                .iter()
                .map(|id| PlanObligation {
                    task_id: (*id).to_owned(),
                    decision: ObligationDecision::Execute,
                    reason: "selected".to_owned(),
                    task_digest: digest.clone(),
                    input_digest: digest.clone(),
                    baseline_proof: None,
                })
                .collect(),
            matrix: PlanMatrix {
                include: Vec::new(),
            },
            task_ids: task_ids.iter().map(|id| (*id).to_owned()).collect(),
            warnings: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// Manifest binding every `task_ids` entry, with advisory freshness.
    fn manifest_with(
        task_ids: &[&str],
        external_data: Option<&ExternalDataFreshness>,
    ) -> BaselineManifest {
        let digest = digest_b3(b"digest");
        BaselineManifest {
            schema: 1,
            repository_id: digest.clone(),
            source_commit: "a".repeat(40),
            ref_: "refs/heads/testmain".to_owned(),
            event: "push".to_owned(),
            workflow_ref: "o/r/.github/workflows/velnor.yml@refs/heads/testmain".to_owned(),
            run_id: 7,
            run_attempt: 1,
            final_status: "passed".to_owned(),
            generator_version: "0.1.0".to_owned(),
            generator_sha256: "1".repeat(64),
            compatibility_id: digest.clone(),
            artifact_id: 9,
            artifact_name: "velnor-baseline".to_owned(),
            expires_at_unix: None,
            tasks: task_ids
                .iter()
                .map(|id| BaselineTaskEntry {
                    task_id: (*id).to_owned(),
                    task_digest: digest.clone(),
                    input_digest: digest.clone(),
                    proof_run_id: 7,
                    observed_run_id: 7,
                    external_data: external_data.cloned(),
                    proof: None,
                })
                .collect(),
        }
    }

    /// Discovery with one plain group per task ID, all unchanged.
    fn discovery_with(task_ids: &[&str]) -> Discovery {
        use velnor_actions_rust::{TaskGroup, TaskKind};
        Discovery {
            statuses: Vec::new(),
            workspaces: Vec::new(),
            task_groups: task_ids
                .iter()
                .map(|id| TaskGroup {
                    task_id: (*id).to_owned(),
                    package_id: "demo".to_owned(),
                    package_name: "demo".to_owned(),
                    manifest_key: "root".to_owned(),
                    kind: TaskKind::Clippy,
                    configuration: "default".to_owned(),
                    features: Vec::new(),
                    target: "host".to_owned(),
                    gated_by: Vec::new(),
                    depends_on: Vec::new(),
                    target_flags: Vec::new(),
                    no_test_targets: false,
                    package_arg: None,
                    compile_driver: "cargo".to_owned(),
                    test_runner: "cargo_nextest".to_owned(),
                    declared_inputs: Vec::new(),
                    undeclared_reads: false,
                    uses_network: false,
                    uses_clock: false,
                    uses_random: false,
                })
                .collect(),
            tool_checks: Vec::new(),
            clippy_memory: crate::clippy_groups::ClippyMemoryPlan {
                groups: Vec::new(),
                barriers: 0,
            },
            recommendations: Vec::new(),
            consumer_manifest_json: None,
        }
    }

    /// Fresh advisory proof over a fixed identity.
    fn fresh_proof() -> ExternalDataFreshness {
        ExternalDataFreshness {
            source: "advisory-db".to_owned(),
            identity: digest_b3(b"snapshot"),
            age_secs: 60,
        }
    }

    #[test]
    fn unknown_schema_and_stale_external_data_stay_execute() {
        let rust = "stack/rust/root/clippy/default";
        let unknown = "stack/unknown/root/test/default";
        let advisory = "stack/rust/root/advisory/default";
        let mut plan = plan_with(&[rust, unknown, advisory]);
        let manifest = manifest_with(&[rust, unknown, advisory], None);
        let unchanged = Some(BTreeSet::new());
        let covered = apply_coverage(
            &mut plan,
            &manifest,
            &digest_b3(b"m"),
            &discovery_with(&[rust, unknown, advisory]),
            unchanged.as_ref(),
        );
        assert_eq!(covered, 1);
        let decision = |id: &str| {
            plan.obligations
                .iter()
                .find(|ob| ob.task_id == id)
                .map(|ob| ob.decision)
        };
        assert_eq!(
            decision(rust),
            Some(ObligationDecision::CoveredByTrustedBaseline)
        );
        assert_eq!(decision(unknown), Some(ObligationDecision::Execute));
        assert_eq!(decision(advisory), Some(ObligationDecision::Execute));
        assert!(
            plan.warnings
                .iter()
                .any(|w| w.contains("unknown_extension_schema"))
        );
        assert!(
            plan.warnings
                .iter()
                .any(|w| w.contains("external_data_rerun"))
        );
    }

    #[test]
    fn fresh_external_data_covers_advisory() {
        let advisory = "stack/rust/root/advisory/default";
        let mut plan = plan_with(&[advisory]);
        let proof = fresh_proof();
        let manifest = manifest_with(&[advisory], Some(&proof));
        let unchanged = Some(BTreeSet::new());
        let covered = apply_coverage(
            &mut plan,
            &manifest,
            &digest_b3(b"m"),
            &discovery_with(&[advisory]),
            unchanged.as_ref(),
        );
        assert_eq!(covered, 1);
        assert_eq!(
            plan.obligations[0].decision,
            ObligationDecision::CoveredByTrustedBaseline
        );
        assert!(plan.obligations[0].baseline_proof.is_some());
    }
}
