//! Removed-path selection: deleted members and cross-package moves.
//!
//! File moves select old and new owners. Deleted manifests and workspace
//! topology changes broaden when base ownership is incomplete.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::impl_common::{TestResult, config_with_branch, fixture_manifest_json, git};
use crate::impl_select::{commit, plan_pr, reasons_for};

/// Nested workspace with a root package plus `members` under `rust/`.
///
/// The nested manifest exercises workspace configuration broadening
/// independently of the repository-root `Cargo.toml` rule.
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
fn reasons_for_root(plan: &velnor_actions_contract::Plan) -> Vec<&str> {
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
    assert!(
        warnings
            .iter()
            .any(|warning| warning == "root_config_changed:all_changed"),
        "workspace topology must broaden: {warnings:?}"
    );
    Ok(())
}

#[test]
fn nested_workspace_inherited_settings_select_all_members() -> TestResult {
    let repo = make_nested_repo(&["alpha", "beta"])?;
    let root = repo.path();
    let base = commit(root, "one")?;
    let manifest = fs::read_to_string(root.join("rust/Cargo.toml"))?;
    fs::write(
        root.join("rust/Cargo.toml"),
        format!("{manifest}\n[profile.dev]\nopt-level = 1\n"),
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    crate::impl_select::assert_all_changed(&plan);
    assert!(
        warnings
            .iter()
            .any(|warning| warning == "root_config_changed:all_changed")
    );
    Ok(())
}

#[test]
fn deleted_manifest_without_base_inventory_broadens() -> TestResult {
    let repo = make_nested_repo(&["alpha", "beta"])?;
    let root = repo.path();
    let retired = root.join("rust/alpha/retired/Cargo.toml");
    fs::create_dir_all(retired.parent().ok_or("missing fixture parent")?)?;
    fs::write(
        &retired,
        "[package]\nname = \"retired\"\nversion = \"0.1.0\"\n",
    )?;
    let base = commit(root, "one")?;
    fs::remove_file(retired)?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    crate::impl_select::assert_all_changed(&plan);
    assert!(
        warnings
            .iter()
            .any(|warning| warning
                == "comparison_unavailable:base_metadata_unavailable:rust/alpha/retired/Cargo.toml:all_changed")
    );
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
