//! Real committed comparisons qualify hints; failures never qualify them.

#[path = "../../test_support/git_fixture.rs"]
mod git_fixture;

use std::fs;
use std::path::Path;

use velnor_actions_contract::WorkflowEvent;

use super::super::{ChangedSelection, classify_changed};
use crate::discover::Discovery;

#[path = "select_evidence_closure_tests.rs"]
mod closure_tests;

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = git_fixture::command(root)
        .map_err(|error| error.to_string())?
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|error| error.to_string())
}

fn commit(root: &Path) -> Result<String, String> {
    git(root, &["add", "."])?;
    git(root, &["commit", "-m", "fixture"])?;
    git(root, &["rev-parse", "HEAD"])
}

fn fixture(owned: bool) -> Result<(tempfile::TempDir, String, Discovery), String> {
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let root = dir.path();
    let package = if owned {
        root.to_path_buf()
    } else {
        root.join("leaf")
    };
    fs::create_dir_all(package.join("src")).map_err(|error| error.to_string())?;
    fs::create_dir_all(root.join(".velnor")).map_err(|error| error.to_string())?;
    fs::write(
        root.join(".velnor/config.toml"),
        "schema = 1\n[workflow]\ndefault_branch = 'testmain'\n",
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        package.join("Cargo.toml"),
        "[package]\nname = 'leaf'\nversion = '0.1.0'\nedition = '2024'\n",
    )
    .map_err(|error| error.to_string())?;
    if !owned {
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = ['leaf']\nresolver = '3'\n",
        )
        .map_err(|error| error.to_string())?;
    }
    fs::write(package.join("src/lib.rs"), "pub fn example() {}\n")
        .map_err(|error| error.to_string())?;
    fs::write(root.join("README.md"), "before\n").map_err(|error| error.to_string())?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n[[package]]\nname = 'leaf'\nversion = '0.1.0'\n",
    )
    .map_err(|error| error.to_string())?;
    git(root, &["init", "-b", "testmain"])?;
    git(root, &["config", "user.email", "test@example.com"])?;
    git(root, &["config", "user.name", "Test"])?;
    git(root, &["config", "commit.gpgsign", "false"])?;
    let base = commit(root)?;
    let discovery = crate::prepare(root)
        .map_err(|error| error.to_string())?
        .discovery;
    Ok((dir, base, discovery))
}

fn classify(
    root: &Path,
    base: &str,
    head: &str,
    discovery: &Discovery,
) -> Result<ChangedSelection, String> {
    classify_changed(
        root,
        WorkflowEvent::PullRequest,
        Some(base),
        head,
        discovery,
        &mut Vec::new(),
    )
    .ok_or_else(|| "comparison unexpectedly unavailable".to_owned())
}

#[test]
fn real_unowned_and_root_owned_paths_both_require_semantic_proof() -> Result<(), String> {
    for owned in [false, true] {
        let (dir, base, discovery) = fixture(owned)?;
        fs::write(dir.path().join("README.md"), "after\n").map_err(|error| error.to_string())?;
        let head = commit(dir.path())?;
        let selection = classify(dir.path(), &base, &head, &discovery)?;
        assert!(!selection.affected.is_empty());
        assert_eq!(selection.proof_refinable, selection.affected);
    }
    Ok(())
}

#[test]
fn local_and_config_comparisons_never_refine() -> Result<(), String> {
    let (dir, base, discovery) = fixture(true)?;
    fs::write(dir.path().join("README.md"), "after\n").map_err(|error| error.to_string())?;
    let local = classify_changed(
        dir.path(),
        WorkflowEvent::Local,
        None,
        &base,
        &discovery,
        &mut Vec::new(),
    )
    .ok_or("local comparison unavailable")?;
    assert!(local.proof_refinable.is_empty());
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = 'leaf'\nversion = '0.2.0'\nedition = '2024'\n",
    )
    .map_err(|error| error.to_string())?;
    let head = commit(dir.path())?;
    assert!(
        classify(dir.path(), &base, &head, &discovery)?
            .proof_refinable
            .is_empty()
    );
    Ok(())
}

#[test]
fn malformed_base_or_candidate_inventory_never_refines() -> Result<(), String> {
    let (dir, base, mut discovery) = fixture(false)?;
    let manifest = dir.path().join("leaf/Cargo.toml");
    let original = fs::read(&manifest).map_err(|error| error.to_string())?;
    fs::write(&manifest, "[malformed").map_err(|error| error.to_string())?;
    let malformed_base = commit(dir.path())?;
    fs::write(&manifest, original).map_err(|error| error.to_string())?;
    fs::write(dir.path().join("README.md"), "after\n").map_err(|error| error.to_string())?;
    let head = commit(dir.path())?;
    assert!(
        classify(dir.path(), &malformed_base, &head, &discovery)?
            .proof_refinable
            .is_empty()
    );
    discovery.workspaces[0].record.packages[0].manifest = "missing/Cargo.toml".to_owned();
    assert!(
        classify(dir.path(), &base, &head, &discovery)?
            .proof_refinable
            .is_empty()
    );
    Ok(())
}
