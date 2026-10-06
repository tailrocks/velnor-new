//! Qualified ownership hints cannot waive consumed source or removed edges.

use std::fs;
use std::path::Path;

use crate::cover_identity::cover_identity_fixtures::OriginalBaseline;
use crate::discover::Discovery;
use crate::internal_plan::closure::resolve_closure_at_root;
use crate::internal_plan::identities::extension_bundle_with_snapshot;
use crate::internal_plan::snapshot::{ExecutionSnapshot, canonical_digest};

use super::{classify, commit, fixture};

fn digest(root: &Path, discovery: &Discovery) -> Result<String, String> {
    let task = discovery
        .proposals
        .iter()
        .find(|task| task.identity.unit_path == "leaf/Cargo.toml" && task.task_kind == "clippy")
        .ok_or("missing leaf clippy proposal")?;
    let snapshot = ExecutionSnapshot::build(discovery).with_checkout(root);
    let bundle = extension_bundle_with_snapshot(&snapshot, discovery, task, Some(root), None);
    let closure = resolve_closure_at_root(
        root,
        task,
        None,
        bundle.graph_digest(),
        "fixed-toolchain",
        "fixed-platform",
        &mut velnor_actions_tofu::FileCache::new(),
        Some(snapshot.checkout_inputs()),
    )
    .map_err(|error| error.to_string())?;
    closure
        .verify_complete()
        .map_err(|error| error.to_string())?;
    canonical_digest(&closure).map_err(|error| error.to_string())
}

#[test]
fn source_plus_unowned_docs_changes_complete_live_closure() -> Result<(), String> {
    let (dir, base, discovery) = fixture(false)?;
    let before = digest(dir.path(), &discovery)?;
    fs::write(dir.path().join("README.md"), "after\n").map_err(|error| error.to_string())?;
    fs::write(dir.path().join("leaf/src/lib.rs"), "pub fn changed() {}\n")
        .map_err(|error| error.to_string())?;
    let head = commit(dir.path())?;
    let candidate = crate::prepare(dir.path())
        .map_err(|error| error.to_string())?
        .discovery;
    let selection = classify(dir.path(), &base, &head, &candidate)?;
    assert!(
        !selection.proof_refinable.is_empty(),
        "hint needs semantic proof"
    );
    assert_ne!(
        before,
        digest(dir.path(), &candidate)?,
        "source forbids baseline identity match"
    );
    Ok(())
}

fn dependency_fixture() -> Result<(tempfile::TempDir, String, Discovery, String), String> {
    let (dir, _, _) = fixture(false)?;
    fs::write(
        dir.path().join("Cargo.toml"),
        "[workspace]\nmembers = ['leaf', 'dependency']\nresolver = '3'\n",
    )
    .map_err(|error| error.to_string())?;
    fs::create_dir_all(dir.path().join("dependency/src")).map_err(|error| error.to_string())?;
    fs::write(
        dir.path().join("dependency/Cargo.toml"),
        "[package]\nname = 'dependency'\nversion = '0.1.0'\nedition = '2024'\n",
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        dir.path().join("dependency/src/lib.rs"),
        "pub fn dependency() {}\n",
    )
    .map_err(|error| error.to_string())?;
    let manifest = dir.path().join("leaf/Cargo.toml");
    let original = fs::read_to_string(&manifest).map_err(|error| error.to_string())?;
    fs::write(
        &manifest,
        format!("{original}[dependencies]\ndependency = {{path = '../dependency'}}\n[dev-dependencies]\ndependency = {{path = '../dependency'}}\n"),
    )
    .map_err(|error| error.to_string())?;
    fs::write(dir.path().join("Cargo.lock"),
        "version = 4\n[[package]]\nname = 'dependency'\nversion = '0.1.0'\n[[package]]\nname = 'leaf'\nversion = '0.1.0'\ndependencies = ['dependency']\n")
        .map_err(|error| error.to_string())?;
    // Fresh discovery establishes the real complete dependency inventory.
    let baseline = crate::prepare(dir.path())
        .map_err(|error| error.to_string())?
        .discovery;
    let base = commit(dir.path())?;
    Ok((dir, base, baseline, original))
}

#[test]
fn removed_dependency_flavor_cannot_match_complete_baseline_closure() -> Result<(), String> {
    let (dir, base, baseline, original) = dependency_fixture()?;
    let manifest = dir.path().join("leaf/Cargo.toml");
    let before = digest(dir.path(), &baseline)?;
    let proof = OriginalBaseline::capture(dir.path(), &baseline, "leaf/Cargo.toml");
    let unchanged = classify(dir.path(), &base, &base, &baseline)?;
    assert_eq!(
        proof.check(dir.path(), &baseline, &unchanged).0,
        1,
        "original authentic complete baseline must first cover"
    );
    let locked = fs::read(dir.path().join("Cargo.lock")).map_err(|error| error.to_string())?;
    fs::write(
        &manifest,
        format!("{original}[dev-dependencies]\ndependency = {{path = '../dependency'}}\n"),
    )
    .map_err(|error| error.to_string())?;
    fs::write(dir.path().join("README.md"), "after\n").map_err(|error| error.to_string())?;
    let candidate = crate::prepare(dir.path())
        .map_err(|error| error.to_string())?
        .discovery;
    let head = commit(dir.path())?;
    assert_eq!(
        locked,
        fs::read(dir.path().join("Cargo.lock")).map_err(|error| error.to_string())?
    );
    let selection = classify(dir.path(), &base, &head, &candidate)?;
    assert!(
        !selection.proof_refinable.is_empty(),
        "qualified graph comparison reached"
    );
    assert!(
        selection.affected.contains(
            &candidate
                .proposals
                .iter()
                .find(|task| task.identity.unit_path == "leaf/Cargo.toml")
                .ok_or("missing consumer")?
                .identity
                .unit_id
        )
    );
    assert_ne!(
        before,
        digest(dir.path(), &candidate)?,
        "removed edge forbids exact proof match"
    );
    let (covered, warnings) = proof.check(dir.path(), &candidate, &selection);
    assert_eq!(
        covered, 0,
        "changed dependency flavor cannot cover: {warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("proof_graph_mismatch")
                || warning.contains("closure_mismatch")),
        "original proof must refuse consumed graph or closure drift: {warnings:?}"
    );
    Ok(())
}
