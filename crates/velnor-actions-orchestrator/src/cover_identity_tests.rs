//! Coverage application tests (P03/P04 matrix unit cases).
//!
//! Declared via `#[path]` from `cover_identity.rs` under `cfg(test)`;
//! fixtures live in the sibling `cover_identity_fixtures` module.

use super::cover_identity_fixtures::*;
use super::*;
use velnor_actions_contract::{WorkflowEvent, canonical_json_bytes};
use velnor_actions_mise::ToolCatalog;

#[test]
fn tofu_tasks_bind_no_mbx_pin_across_catalogs() {
    use velnor_actions_mise::PinnedTool;
    use velnor_actions_tofu::{TofuTaskGroup, TofuTaskKind};
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
    let tofu = velnor_actions_tofu::propose_task(&group).expect("proposes");
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

/// Live proof dimensions for `task_id`: graph, toolchain, platform.
fn live_proof_dims(
    root: &std::path::Path,
    discovery: &Discovery,
    task_id: &str,
    catalog: &ToolCatalog,
    label: &str,
) -> (String, String, String) {
    let task = discovery
        .proposals
        .iter()
        .find(|task| task.task_id == task_id)
        .expect("task");
    let snapshot = ExecutionSnapshot::build(discovery);
    let bundle = extension_bundle_with_snapshot(
        &snapshot,
        discovery,
        task,
        Some(root),
        nextest_config_for(discovery, task).as_deref(),
    );
    (
        bundle.graph_digest().to_owned(),
        toolchain_id(task, catalog).expect("toolchain"),
        platform_id_for_group(label, task).expect("platform"),
    )
}

/// Structured proof over the entry digests with live identity dims.
fn live_proof(
    task_id: &str,
    digest: &str,
    graph: &str,
    toolchain: &str,
    platform: &str,
    mbx: &str,
) -> velnor_actions_contract::ManifestTaskProof {
    velnor_actions_contract::ManifestTaskProof::new(
        task_id, digest, digest, graph, toolchain, mbx, platform, "default", 7,
    )
    .expect("proof")
}

fn live_mbx(discovery: &Discovery, task_id: &str, catalog: &ToolCatalog) -> String {
    let task = discovery
        .proposals
        .iter()
        .find(|task| task.task_id == task_id)
        .expect("task");
    super::live_mbx_digest(task, catalog)
}

/// Structured proofs compare their carried identity against live
/// values: drifted graph, toolchain, or platform refuses coverage,
/// while a matching proof covers and records the mbx/profile gap.
#[test]
fn structured_proof_dimensions_compare_live() {
    let rust = "stack/rust/root/clippy/default";
    let digest = velnor_actions_contract::digest_b3(b"digest");
    let unchanged = Some(BTreeSet::new());
    let tmp = tempfile::tempdir().expect("tempdir");
    seed_sources(tmp.path());
    let catalog = ToolCatalog::pinned();
    let discovery = discovery_with(&[rust]);
    let live = live_closure_digest(tmp.path(), &discovery, rust, &catalog);
    let (graph, toolchain, platform) =
        live_proof_dims(tmp.path(), &discovery, rust, &catalog, "ubuntu-26.04");
    let contracted = digest.clone();
    let cover = |proof: velnor_actions_contract::ManifestTaskProof| {
        let mut manifest = manifest_with(&[(rust, &live)]);
        manifest.tasks[0].proof = Some(proof);
        let mut plan = plan_with(&[rust]);
        let covered = apply_coverage(
            &mut plan,
            &manifest,
            &provenance_for(&manifest),
            &discovery,
            unchanged.as_ref(),
            &inputs(tmp.path(), &catalog),
        );
        (covered, plan.warnings)
    };
    let mbx = live_mbx(&discovery, rust, &catalog);
    let (covered, warnings) = cover(live_proof(
        rust, &digest, &graph, &toolchain, &platform, &mbx,
    ));
    assert_eq!(covered, 1, "{warnings:?}");
    assert!(
        warnings.iter().all(|w| !w.contains("proof_")),
        "{warnings:?}"
    );
    for (label, proof) in [
        (
            "proof_graph_mismatch",
            live_proof(
                rust,
                &contracted,
                &velnor_actions_contract::digest_b3(b"other-graph"),
                &toolchain,
                &platform,
                &mbx,
            ),
        ),
        (
            "proof_toolchain_mismatch",
            live_proof(
                rust,
                &contracted,
                &graph,
                &velnor_actions_contract::digest_b3(b"other-toolchain"),
                &platform,
                &mbx,
            ),
        ),
        (
            "proof_platform_mismatch",
            live_proof(
                rust,
                &contracted,
                &graph,
                &toolchain,
                &velnor_actions_contract::digest_b3(b"other-platform"),
                &mbx,
            ),
        ),
    ] {
        let (covered, warnings) = cover(proof);
        assert_eq!(covered, 0, "{label}");
        assert!(
            warnings.iter().any(|w| w.contains(label)),
            "{label}: {warnings:?}"
        );
    }
}

#[test]
fn baseline_application_returns_the_exact_manifest_used_for_coverage() {
    let rust = "stack/rust/root/clippy/default";
    let mut plan = plan_with(&[rust]);
    plan.base = Some("a".repeat(40));
    let tmp = tempfile::tempdir().expect("tempdir");
    seed_sources(tmp.path());
    let catalog = ToolCatalog::pinned();
    let discovery = discovery_with(&[rust]);
    let live = live_closure_digest(tmp.path(), &discovery, rust, &catalog);
    let manifest = manifest_with(&[(rust, &live)]);
    let expected = canonical_json_bytes(&manifest).expect("manifest bytes");
    let inputs = BaselineInputs {
        branch: "testmain",
        root: tmp.path(),
        workflow: ".github/workflows/ci.yml",
        catalog: &catalog,
        repository: Some("o/r"),
    };
    let used = crate::cover::apply_baseline(
        &mut plan,
        WorkflowEvent::PullRequest,
        inputs,
        Some(manifest),
        &discovery,
        Some(&BTreeSet::new()),
    )
    .expect("apply baseline")
    .expect("coverage source retained");
    assert_eq!(canonical_json_bytes(&used).expect("used bytes"), expected);
    assert_eq!(
        plan.obligations[0].decision,
        ObligationDecision::CoveredByTrustedBaseline
    );
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

include!("cover_identity_tests.part2.rs");
