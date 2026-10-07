//! Coverage application tests (P03/P04 matrix unit cases).
//!
//! Declared via `#[path]` from `cover_identity.rs` under `cfg(test)`;
//! fixtures live in the sibling `cover_identity_fixtures` module.

use super::*;

use velnor_actions_mise::ToolCatalog;

#[test]
fn tofu_tasks_bind_no_mbx_pin_across_catalogs() {
    use velnor_actions_mise::PinnedTool;
    use velnor_actions_tofu_core::{TofuTaskGroup, TofuTaskKind};
    let pinned = ToolCatalog::pinned();
    let bumped = ToolCatalog::new(
        pinned.version(PinnedTool::Rust),
        "9.9.9",
        pinned.version(PinnedTool::Gh),
        pinned.version(PinnedTool::Actionlint),
        pinned.version(PinnedTool::Shellcheck),
        pinned.version(PinnedTool::Zizmor),
        pinned.version(PinnedTool::Nextest),
        pinned.version(PinnedTool::Opentofu),
    )
    .expect("catalog");
    let group = TofuTaskGroup {
        root: String::new(),
        kind: TofuTaskKind::Validate,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let tofu = velnor_actions_tofu_core::propose_task(&group).expect("proposes");
    assert_eq!(
        live_mbx_digest(&tofu, &pinned),
        live_mbx_digest(&tofu, &bumped),
        "tofu binds no mbx pin"
    );
    let mut mbx = tofu.clone();
    mbx.stack_id = "rust".to_owned();
    mbx.identity.compile_driver = "mbx".to_owned();
    assert_ne!(
        live_mbx_digest(&mbx, &pinned),
        live_mbx_digest(&mbx, &bumped),
        "control: mbx-driven tasks bind the pin"
    );
}

#[test]
fn undiscovered_and_unknown_groups_never_cover() {
    let rust = "stack/rust/root/clippy/default";
    let unknown = "stack/unknown/root/test/default";
    let mut plan = plan_with(&[rust, unknown]);
    let stale = stale_closure();
    let manifest = manifest_with(&[(rust, &stale), (unknown, &stale)]);
    let unchanged = Some(BTreeSet::new());
    let tmp = tempfile::tempdir().expect("tempdir");
    let catalog = ToolCatalog::pinned();
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance_for(&manifest),
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
fn matching_closure_covers() {
    let rust = "stack/rust/root/clippy/default";
    let mut plan = plan_with(&[rust]);
    let unchanged = Some(BTreeSet::new());
    let tmp = tempfile::tempdir().expect("tempdir");
    seed_sources(tmp.path());
    let catalog = ToolCatalog::pinned();
    let discovery = discovery_with(&[rust]);
    let live = live_closure_digest(tmp.path(), &discovery, rust, &catalog);
    let manifest = manifest_with(&[(rust, &live)]);
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance_for(&manifest),
        &discovery,
        unchanged.as_ref(),
        &inputs(tmp.path(), &catalog),
    );
    assert_eq!(covered, 1);
    assert_eq!(
        plan.obligations[0].decision,
        ObligationDecision::CoveredByTrustedBaseline
    );
    assert!(plan.obligations[0].baseline_proof.is_some());
}

#[test]
fn stale_closure_with_empty_changed_set_executes() {
    let rust = "stack/rust/root/clippy/default";
    let unchanged = Some(BTreeSet::new());
    let tmp = tempfile::tempdir().expect("tempdir");
    seed_sources(tmp.path());
    let catalog = ToolCatalog::pinned();
    let discovery = discovery_with(&[rust]);
    let before = live_closure_digest(tmp.path(), &discovery, rust, &catalog);
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )
    .expect("edit");
    let after = live_closure_digest(tmp.path(), &discovery, rust, &catalog);
    assert_ne!(before, after, "the edit must flip the live closure");
    // Task and input digests still match and the changed set is empty:
    // only the closure comparison refuses coverage.
    let manifest = manifest_with(&[(rust, &before)]);
    let mut plan = plan_with(&[rust]);
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance_for(&manifest),
        &discovery,
        unchanged.as_ref(),
        &inputs(tmp.path(), &catalog),
    );
    assert_eq!(covered, 0);
    assert_eq!(plan.obligations[0].decision, ObligationDecision::Execute);
    assert!(
        plan.warnings.iter().any(|w| w.contains("closure_mismatch")),
        "{:?}",
        plan.warnings
    );
}

#[test]
fn incomplete_closure_refuses() {
    let rust = "stack/rust/root/clippy/default";
    let mut broken = discovery_with(&[rust]);
    broken.proposals[0]
        .identity
        .declared_inputs
        .push("missing/input.proto".to_owned());
    let stale = stale_closure();
    let manifest = manifest_with(&[(rust, &stale)]);
    let unchanged = Some(BTreeSet::new());
    let tmp = tempfile::tempdir().expect("tempdir");
    seed_sources(tmp.path());
    let catalog = ToolCatalog::pinned();
    let mut plan = plan_with(&[rust]);
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance_for(&manifest),
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
    dirty.proposals[0].identity.undeclared_reads = true;
    let mut plan = plan_with(&[rust]);
    let stale = stale_closure();
    let manifest = manifest_with(&[(rust, &stale)]);
    let unchanged = Some(BTreeSet::new());
    let tmp = tempfile::tempdir().expect("tempdir");
    let catalog = ToolCatalog::pinned();
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance_for(&manifest),
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
    let stale = stale_closure();
    let manifest = manifest_with(&[(rust, &stale)]);
    let changed = Some(BTreeSet::from(["demo".to_owned()]));
    let tmp = tempfile::tempdir().expect("tempdir");
    let catalog = ToolCatalog::pinned();
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance_for(&manifest),
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
    let unchanged = Some(BTreeSet::new());
    let tmp = tempfile::tempdir().expect("tempdir");
    seed_sources(tmp.path());
    let catalog = ToolCatalog::pinned();
    let discovery = discovery_with(&[advisory]);
    let live = live_closure_digest(tmp.path(), &discovery, advisory, &catalog);
    let manifest = manifest_with(&[(advisory, &live)]);
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance_for(&manifest),
        &discovery,
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

/// Tofu proposal via the T12 adapter constructor.
fn tofu_proposal(kind: velnor_actions_tofu_core::TofuTaskKind) -> ProposedTask {
    let group = velnor_actions_tofu_core::TofuTaskGroup {
        root: String::new(),
        kind,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu_core::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

/// Cover-time bundle for one task over an empty discovery.
fn cover_bundle_for(
    snapshot: &ExecutionSnapshot,
    discovery: &Discovery,
    task: &ProposedTask,
    root: &std::path::Path,
) -> velnor_actions_orchestrator_graph::internal_plan::identities::ExtensionBundle {
    extension_bundle_with_snapshot(snapshot, discovery, task, Some(root), None)
}

/// Valid tofu spellings verify at cover time (G5 close).
#[test]
fn tofu_cover_extension_verifies_valid_spellings() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join(".terraform.lock.hcl"), "lock").expect("lockfile");
    let task = tofu_proposal(TofuTaskKind::Validate);
    let discovery = discovery_with(&[]);
    let snapshot = ExecutionSnapshot::build(&discovery);
    let bundle = cover_bundle_for(&snapshot, &discovery, &task, tmp.path());
    let mut reads = velnor_actions_tofu_core::FileCache::new();
    verify_cover_extension(&task, tmp.path(), &bundle, &mut reads)
        .expect("tofu verifies at cover time");
}

/// Drifted tofu spellings refuse coverage, never parse loosely.
#[test]
fn tofu_cover_extension_refuses_drift() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut task = tofu_proposal(TofuTaskKind::InitForValidate);
    task.identity.compile_driver = "cargo".to_owned();
    let discovery = discovery_with(&[]);
    let snapshot = ExecutionSnapshot::build(&discovery);
    let bundle = cover_bundle_for(&snapshot, &discovery, &task, tmp.path());
    let mut reads = velnor_actions_tofu_core::FileCache::new();
    let err =
        verify_cover_extension(&task, tmp.path(), &bundle, &mut reads).expect_err("drift refuses");
    assert!(err.contains("unparsable_spelling"), "{err}");
}

/// Undeclared tofu inputs refuse coverage like undeclared rust reads.
#[test]
fn tofu_cover_extension_refuses_undeclared_inputs() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut task = tofu_proposal(TofuTaskKind::Validate);
    task.identity.undeclared_reads = true;
    let discovery = discovery_with(&[]);
    let snapshot = ExecutionSnapshot::build(&discovery);
    let bundle = cover_bundle_for(&snapshot, &discovery, &task, tmp.path());
    let mut reads = velnor_actions_tofu_core::FileCache::new();
    let err = verify_cover_extension(&task, tmp.path(), &bundle, &mut reads)
        .expect_err("undeclared refuses");
    assert_eq!(err, "undeclared_inputs");
}
