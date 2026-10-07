//! Removed-path selection: deleted members and cross-package moves.
//!
//! Ownership resolves against head manifest directories by path prefix,
//! so paths deleted at head still select their surviving owners (and
//! the base graph still resolves removed edges against head IDs).

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::impl_select::{commit, plan_pr, reasons_for};
use crate::support::{TestResult, config_with_branch, fixture_manifest_json, git};

/// Nested workspace with a root package plus `members` under `rust/`.
///
/// The nested manifest keeps member add/remove edits out of the
/// repo-root `Cargo.toml` broaden rule, so structural removals flow
/// through the base/head graphs instead of broadening.
fn make_nested_repo(members: &[&str]) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config_with_branch())?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    fs::create_dir_all(root.join("rust/src"))?;
    write_nested_manifest(root, members)?;
    fs::write(root.join("rust/src/lib.rs"), "pub fn f() {}\n")?;
    for member in members {
        let dir = root.join("rust").join(member);
        fs::create_dir_all(dir.join("src"))?;
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{member}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )?;
        fs::write(dir.join("src/lib.rs"), "pub fn f() {}\n")?;
    }
    Ok(dir)
}

/// Rewrite the nested workspace manifest with a root package.
fn write_nested_manifest(root: &Path, members: &[&str]) -> TestResult {
    let list = members
        .iter()
        .map(|m| format!("\"{m}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        root.join("rust/Cargo.toml"),
        format!(
            "[package]\nname = \"rootpkg\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\nmembers = [{list}]\n"
        ),
    )?;
    Ok(())
}

/// Reasons of the root package's obligations, keyed by manifest dir.
///
/// Root tasks carry the workspace directory (`stack/rust/rust/...`)
/// instead of the package name, with one fewer path segment than
/// member tasks.
fn reasons_for_root(plan: &velnor_actions_contract_workflow::Plan) -> Vec<&str> {
    let reasons: Vec<&str> = plan
        .obligations
        .iter()
        .filter(|ob| {
            ob.task_id.starts_with("stack/rust/rust/") && ob.task_id.split('/').count() == 5
        })
        .map(|ob| ob.reason.as_str())
        .collect();
    assert!(!reasons.is_empty(), "root package has obligations");
    reasons
}

#[test]
fn deleted_member_selects_surviving_owners() -> TestResult {
    let repo = make_nested_repo(&["alpha", "beta"])?;
    let root = repo.path();
    fs::write(
        root.join("rust/alpha/Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nbeta = { path = \"../beta\" }\n",
    )?;
    let base = commit(root, "one")?;
    fs::remove_dir_all(root.join("rust/beta"))?;
    write_nested_manifest(root, &["alpha"])?;
    fs::write(
        root.join("rust/alpha/Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(
        !plan.task_ids.iter().any(|id| id.contains("beta")),
        "deleted member leaves no tasks: {:?}",
        plan.task_ids
    );
    let rootpkg = reasons_for_root(&plan);
    assert!(
        rootpkg.iter().all(|r| *r == "affected_by_change"),
        "root package owns the deleted paths: {rootpkg:?}"
    );
    let alpha = reasons_for(&plan, "alpha");
    assert!(
        alpha.iter().all(|r| *r == "affected_by_change"),
        "dependent manifest changed: {alpha:?}"
    );
    assert!(warnings.is_empty(), "narrow, got {warnings:?}");
    Ok(())
}

#[test]
fn moved_file_selects_old_and_new_owners() -> TestResult {
    let repo = make_nested_repo(&["alpha", "beta"])?;
    let root = repo.path();
    fs::write(
        root.join("rust/alpha/src/shared.rs"),
        "pub fn shared() {}\n",
    )?;
    let base = commit(root, "one")?;
    git(
        &["mv", "rust/alpha/src/shared.rs", "rust/beta/src/shared.rs"],
        root,
    )?;
    fs::write(
        root.join("rust/beta/src/shared.rs"),
        "pub fn relocated() {}\npub fn extra_one() {}\npub fn extra_two() {}\npub fn extra_three() {}\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    let alpha = reasons_for(&plan, "alpha");
    assert!(
        alpha.iter().all(|r| *r == "affected_by_change"),
        "old-path owner stays selected: {alpha:?}"
    );
    let beta = reasons_for(&plan, "beta");
    assert!(
        beta.iter().all(|r| *r == "affected_by_change"),
        "new-path owner selected: {beta:?}"
    );
    assert!(warnings.is_empty(), "narrow, got {warnings:?}");
    Ok(())
}

#[test]
fn pure_rename_selects_old_and_new_owners() -> TestResult {
    let repo = make_nested_repo(&["alpha", "beta"])?;
    let root = repo.path();
    fs::write(
        root.join("rust/alpha/src/shared.rs"),
        "pub fn shared() {}\n",
    )?;
    let base = commit(root, "one")?;
    // No modification: git reports an R100 rename, which default
    // `--name-only` collapses to the new path only.
    git(
        &["mv", "rust/alpha/src/shared.rs", "rust/beta/src/shared.rs"],
        root,
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    let alpha = reasons_for(&plan, "alpha");
    assert!(
        alpha.iter().all(|r| *r == "affected_by_change"),
        "old-path owner stays selected: {alpha:?}"
    );
    let beta = reasons_for(&plan, "beta");
    assert!(
        beta.iter().all(|r| *r == "affected_by_change"),
        "new-path owner selected: {beta:?}"
    );
    assert!(warnings.is_empty(), "narrow, got {warnings:?}");
    Ok(())
}
