//! Structured-proof comparison tests.
//!
//! Declared via `#[path]` from `cover_identity.rs` under `cfg(test)`;
//! fixtures live in the sibling `cover_identity_fixtures` module.

use super::*;

use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust_core::CompileDriver;

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
    profile: &str,
    mbx: &str,
) -> velnor_actions_contract::ManifestTaskProof {
    velnor_actions_contract::ManifestTaskProof::new(
        task_id, digest, digest, graph, toolchain, mbx, platform, profile, 7,
    )
    .expect("proof")
}

/// Live closure plus all five live proof dims for the fixture group.
#[derive(Debug)]
struct LiveFixture {
    /// Scratch checkout root backing live resolution.
    tmp: tempfile::TempDir,
    /// Discovery with the single fixture group.
    discovery: Discovery,
    /// Live closure digest the entries bind.
    live: String,
    /// Live graph digest.
    graph: String,
    /// Live toolchain identity.
    toolchain: String,
    /// Live platform identity.
    platform: String,
    /// Live execution profile.
    profile: String,
    /// Live mbx dimension.
    mbx: String,
}

/// Live fixture plus the pinned catalog behind it.
fn live_fixture() -> (LiveFixture, ToolCatalog) {
    let rust = "stack/rust/root/clippy/default";
    let tmp = tempfile::tempdir().expect("tempdir");
    seed_sources(tmp.path());
    let catalog = ToolCatalog::pinned();
    let discovery = discovery_with(&[rust]);
    let live = live_closure_digest(tmp.path(), &discovery, rust, &catalog);
    let (graph, toolchain, platform) =
        live_proof_dims(tmp.path(), &discovery, rust, &catalog, "ubuntu-26.04");
    let task = discovery
        .proposals
        .iter()
        .find(|task| task.task_id == rust)
        .expect("task");
    let profile = task.configuration.clone();
    let mbx = live_mbx_digest(task, &catalog);
    (
        LiveFixture {
            tmp,
            discovery,
            live,
            graph,
            toolchain,
            platform,
            profile,
            mbx,
        },
        catalog,
    )
}

/// Coverage verdict for one carried proof over the live fixture.
fn cover_with(
    fixture: &LiveFixture,
    catalog: &ToolCatalog,
    proof: velnor_actions_contract::ManifestTaskProof,
) -> (u32, Vec<String>) {
    let rust = "stack/rust/root/clippy/default";
    let mut manifest = manifest_with(&[(rust, &fixture.live)]);
    manifest.tasks[0].proof = Some(proof);
    let mut plan = plan_with(&[rust]);
    let unchanged = BTreeSet::new();
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance_for(&manifest),
        &fixture.discovery,
        Some(&unchanged),
        &inputs(fixture.tmp.path(), catalog),
    );
    (covered, plan.warnings)
}

/// A proof matching all five live dims covers silently: no gap note,
/// no miss warning.
#[test]
fn structured_proof_match_covers_silently() {
    let rust = "stack/rust/root/clippy/default";
    let digest = velnor_actions_contract::digest_b3(b"digest");
    let (fixture, catalog) = live_fixture();
    let proof = live_proof(
        rust,
        &digest,
        &fixture.graph,
        &fixture.toolchain,
        &fixture.platform,
        &fixture.profile,
        &fixture.mbx,
    );
    let (covered, warnings) = cover_with(&fixture, &catalog, proof);
    assert_eq!(covered, 1);
    assert!(
        warnings.iter().all(|w| !w.contains("baseline_note")),
        "a matching proof covers silently: {warnings:?}"
    );
}

/// One drifted proof per dimension, each labeled by its miss token.
fn drifted_proofs(
    rust: &str,
    digest: &str,
    fixture: &LiveFixture,
) -> Vec<(&'static str, velnor_actions_contract::ManifestTaskProof)> {
    let LiveFixture {
        graph,
        toolchain,
        platform,
        profile,
        mbx,
        ..
    } = fixture;
    let other = |seed: &[u8]| velnor_actions_contract::digest_b3(seed);
    vec![
        (
            "proof_graph_mismatch",
            live_proof(
                rust,
                digest,
                &other(b"other-graph"),
                toolchain,
                platform,
                profile,
                mbx,
            ),
        ),
        (
            "proof_toolchain_mismatch",
            live_proof(
                rust,
                digest,
                graph,
                &other(b"other-toolchain"),
                platform,
                profile,
                mbx,
            ),
        ),
        (
            "proof_platform_mismatch",
            live_proof(
                rust,
                digest,
                graph,
                toolchain,
                &other(b"other-platform"),
                profile,
                mbx,
            ),
        ),
        (
            "proof_profile_mismatch",
            live_proof(
                rust,
                digest,
                graph,
                toolchain,
                platform,
                "other-profile",
                mbx,
            ),
        ),
        (
            "proof_mbx_mismatch",
            live_proof(
                rust,
                digest,
                graph,
                toolchain,
                platform,
                profile,
                &other(b"other-mbx"),
            ),
        ),
    ]
}

/// Drift in any one of the five carried dims refuses coverage with
/// its own miss token.
#[test]
fn structured_proof_drift_refuses_per_dimension() {
    let rust = "stack/rust/root/clippy/default";
    let digest = velnor_actions_contract::digest_b3(b"digest");
    let (fixture, catalog) = live_fixture();
    for (label, proof) in drifted_proofs(rust, &digest, &fixture) {
        let (covered, warnings) = cover_with(&fixture, &catalog, proof);
        assert_eq!(covered, 0, "{label}");
        assert!(
            warnings.iter().any(|w| w.contains(label)),
            "{label}: {warnings:?}"
        );
    }
}

/// The live mbx dimension binds the driver plus the catalog pin for
/// mbx-driven groups only: cargo groups ignore pin drift, mbx groups
/// refuse proofs from another driver.
#[test]
fn live_mbx_binds_driver_and_pin() {
    let catalog = ToolCatalog::pinned();
    let discovery = discovery_with(&["stack/rust/root/clippy/default"]);
    let cargo = &discovery.proposals[0];
    assert_eq!(cargo.identity.compile_driver, CompileDriver::Cargo.as_str());
    let mut mbx = cargo.clone();
    mbx.identity.compile_driver = CompileDriver::Mbx.as_str().to_owned();
    assert_ne!(
        live_mbx_digest(cargo, &catalog),
        live_mbx_digest(&mbx, &catalog),
        "driver drift changes the live mbx dim"
    );
    let mut renamed = cargo.clone();
    renamed.task_id = "stack/rust/root/other/default".to_owned();
    assert_eq!(
        live_mbx_digest(cargo, &catalog),
        live_mbx_digest(&renamed, &catalog),
        "unrelated group facts leave the live mbx dim alone"
    );
}
