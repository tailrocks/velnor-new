//! P10 NUL-path cases: exact path bytes (Unicode, spaces, newlines,
//! deletes, renames, staged/unstaged union, non-UTF-8) through the typed
//! plan boundary.

use std::fs;
use std::path::Path;

use velnor_actions_contract::Plan;
use velnor_actions_orchestrator::{plan_internal, resolve_root};

use crate::impl_common::{TestResult, git};
use crate::impl_select::{commit, make_ws_repo, plan_pr, reasons_for};

/// Plan the working tree against `HEAD` as a local run.
fn plan_local(root: &Path, head: &str) -> Result<(Plan, Vec<String>), Box<dyn std::error::Error>> {
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": None::<String>,
        "head": head,
        "event": "local",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    let warnings: Vec<String> = serde_json::from_value(value["plan"]["warnings"].clone())?;
    Ok((plan, warnings))
}

/// Assert one member changed and the other stayed forced-uncached.
fn assert_narrow(plan: &Plan, changed: &str, other: &str) {
    let hit = reasons_for(plan, changed);
    assert!(
        hit.iter().all(|r| *r == "affected_by_change"),
        "{changed}: {hit:?}"
    );
    let miss = reasons_for(plan, other);
    assert!(
        miss.iter().all(|r| *r == "forced_uncached"),
        "{other}: {miss:?}"
    );
}

#[test]
fn unicode_filename_selects_owner_only() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    fs::write(root.join("alpha/src/héllo.rs"), "pub fn f() {}\n")?;
    let base = commit(root, "one")?;
    fs::write(
        root.join("alpha/src/héllo.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert_narrow(&plan, "alpha", "beta");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn trailing_space_filename_selects_owner_only() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    fs::write(root.join("alpha/src/trailing.rs "), "pub fn f() {}\n")?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert_narrow(&plan, "alpha", "beta");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn whitespace_only_filename_not_dropped() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    fs::write(root.join("   "), "notes\n")?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(
        plan.task_ids.iter().any(|id| id.contains("alpha"))
            && plan.task_ids.iter().any(|id| id.contains("beta")),
        "unowned change broadens: {:?}",
        plan.task_ids
    );
    assert!(
        warnings.iter().any(|w| w.contains("unclassified_files")),
        "tag: {warnings:?}"
    );
    Ok(())
}

#[test]
fn newline_filename_selects_owner_only() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    fs::write(root.join("alpha/src/with\nnewline.rs"), "pub fn f() {}\n")?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert_narrow(&plan, "alpha", "beta");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn deleted_path_selects_owner_only() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    fs::write(root.join("beta/src/extra.rs"), "pub fn extra() {}\n")?;
    let base = commit(root, "one")?;
    fs::remove_file(root.join("beta/src/extra.rs"))?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert_narrow(&plan, "beta", "alpha");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn renamed_path_selects_owner_only() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    fs::write(root.join("beta/src/extra.rs"), "pub fn extra() {}\n")?;
    let base = commit(root, "one")?;
    git(&["mv", "beta/src/extra.rs", "beta/src/renamed.rs"], root)?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert_narrow(&plan, "beta", "alpha");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn local_staged_and_unstaged_union_selects() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let head = commit(root, "one")?;
    fs::write(
        root.join("alpha/src/lib.rs"),
        "pub fn f() {}\npub fn a() {}\n",
    )?;
    fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn b() {}\n",
    )?;
    git(&["add", "beta/src/lib.rs"], root)?;
    let (plan, warnings) = plan_local(root, &head)?;
    for member in ["alpha", "beta"] {
        let reasons = reasons_for(&plan, member);
        assert!(
            reasons.iter().all(|r| *r == "affected_by_change"),
            "{member}: {reasons:?}"
        );
    }
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn local_unicode_change_selects_owner_only() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    fs::write(root.join("alpha/src/héllo.rs"), "pub fn f() {}\n")?;
    let head = commit(root, "one")?;
    fs::write(
        root.join("alpha/src/héllo.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let (plan, warnings) = plan_local(root, &head)?;
    assert_narrow(&plan, "alpha", "beta");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn nested_subdir_resolves_repo_root() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    commit(root, "one")?;
    let subdir = root.join("alpha/src");
    assert_eq!(resolve_root(&subdir)?, root.canonicalize()?);
    Ok(())
}

#[test]
#[cfg(unix)]
fn non_utf8_path_broadens_explicitly() -> TestResult {
    use std::os::unix::ffi::OsStrExt;
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    let raw = b"alpha/src/\xffinvalid.rs";
    let path = root.join(std::ffi::OsStr::from_bytes(raw));
    if fs::write(&path, "pub fn f() {}\n").is_err() {
        return Ok(());
    }
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(
        plan.task_ids.iter().any(|id| id.contains("alpha"))
            && plan.task_ids.iter().any(|id| id.contains("beta")),
        "broadens: {:?}",
        plan.task_ids
    );
    assert!(
        warnings.iter().any(|w| w.contains("non_utf8_path")),
        "explicit tag: {warnings:?}"
    );
    Ok(())
}
