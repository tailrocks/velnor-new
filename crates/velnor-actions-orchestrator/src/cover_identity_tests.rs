//! Coverage application tests (P03/P04 matrix unit cases).
//!
//! Declared via `#[path]` from `cover_identity.rs` under `cfg(test)`.

use super::*;
use crate::merge::required_evidence::BaselineTaskEntry;
use velnor_actions_contract::{
    BaselineStatus, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner,
    RunnerSelection, Trust, WorkflowEvent,
};
use velnor_actions_mise::ToolCatalog;

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
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "1".repeat(64),
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

/// Manifest binding every `task_ids` entry.
fn manifest_with(task_ids: &[&str]) -> BaselineManifest {
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
                external_data: None,
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
                test_runner: "cargo_test".to_owned(),
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

/// Baseline inputs over a temp checkout root.
fn inputs<'a>(root: &'a std::path::Path, catalog: &'a ToolCatalog) -> BaselineInputs<'a> {
    BaselineInputs {
        branch: "testmain",
        root,
        workflow: ".github/workflows/velnor.yml",
        catalog,
    }
}

/// Validated provenance for coverage tests.
fn provenance() -> ValidatedProvenance {
    ValidatedProvenance {
        source_commit: "a".repeat(40),
        run_id: 7,
        artifact_name: "velnor-baseline".to_owned(),
        artifact_id: 9,
        manifest_digest: digest_b3(b"m"),
    }
}

#[test]
fn undiscovered_and_unknown_groups_never_cover() {
    let rust = "stack/rust/root/clippy/default";
    let unknown = "stack/unknown/root/test/default";
    let mut plan = plan_with(&[rust, unknown]);
    let manifest = manifest_with(&[rust, unknown]);
    let unchanged = Some(BTreeSet::new());
    let tmp = tempfile::tempdir().expect("tempdir");
    let catalog = ToolCatalog::pinned();
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance(),
        &discovery_with(&[]),
        unchanged.as_ref(),
        &inputs(tmp.path(), &catalog),
    );
    assert_eq!(covered, 0);
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute)
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("undiscovered_task_group")),
        "{:?}",
        plan.warnings
    );
}

#[test]
fn complete_closure_covers_but_incomplete_refuses() {
    let rust = "stack/rust/root/clippy/default";
    let mut plan = plan_with(&[rust]);
    let manifest = manifest_with(&[rust]);
    let unchanged = Some(BTreeSet::new());
    let tmp = tempfile::tempdir().expect("tempdir");
    let catalog = ToolCatalog::pinned();
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance(),
        &discovery_with(&[rust]),
        unchanged.as_ref(),
        &inputs(tmp.path(), &catalog),
    );
    assert_eq!(covered, 1);
    assert_eq!(
        plan.obligations[0].decision,
        ObligationDecision::CoveredByTrustedBaseline
    );
    assert!(plan.obligations[0].baseline_proof.is_some());
    let mut broken = discovery_with(&[rust]);
    broken.task_groups[0]
        .declared_inputs
        .push("missing/input.proto".to_owned());
    let mut plan = plan_with(&[rust]);
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance(),
        &broken,
        unchanged.as_ref(),
        &inputs(tmp.path(), &catalog),
    );
    assert_eq!(covered, 0);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("incomplete_inputs")),
        "{:?}",
        plan.warnings
    );
}

#[test]
fn undeclared_reads_refuse_with_warning() {
    let rust = "stack/rust/root/clippy/default";
    let mut dirty = discovery_with(&[rust]);
    dirty.task_groups[0].undeclared_reads = true;
    let mut plan = plan_with(&[rust]);
    let manifest = manifest_with(&[rust]);
    let unchanged = Some(BTreeSet::new());
    let tmp = tempfile::tempdir().expect("tempdir");
    let catalog = ToolCatalog::pinned();
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance(),
        &dirty,
        unchanged.as_ref(),
        &inputs(tmp.path(), &catalog),
    );
    assert_eq!(covered, 0);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("undeclared_inputs")),
        "{:?}",
        plan.warnings
    );
}

#[test]
fn changed_work_executes_despite_identity_match() {
    let rust = "stack/rust/root/clippy/default";
    let mut plan = plan_with(&[rust]);
    let manifest = manifest_with(&[rust]);
    let changed = Some(BTreeSet::from(["demo".to_owned()]));
    let tmp = tempfile::tempdir().expect("tempdir");
    let catalog = ToolCatalog::pinned();
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance(),
        &discovery_with(&[rust]),
        changed.as_ref(),
        &inputs(tmp.path(), &catalog),
    );
    assert_eq!(covered, 0);
    assert_eq!(plan.obligations[0].decision, ObligationDecision::Execute);
}

#[test]
fn advisory_needs_fresh_external_data() {
    let advisory = "stack/rust/root/advisory/default";
    let mut plan = plan_with(&[advisory]);
    let manifest = manifest_with(&[advisory]);
    let unchanged = Some(BTreeSet::new());
    let tmp = tempfile::tempdir().expect("tempdir");
    let catalog = ToolCatalog::pinned();
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance(),
        &discovery_with(&[advisory]),
        unchanged.as_ref(),
        &inputs(tmp.path(), &catalog),
    );
    assert_eq!(covered, 0);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("external_data_rerun")),
        "{:?}",
        plan.warnings
    );
}
