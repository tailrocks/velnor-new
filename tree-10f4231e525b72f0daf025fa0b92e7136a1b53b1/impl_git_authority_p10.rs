//! P10 Git-authority cases: origin identity through `git config` and
//! NUL-delimited path handling through the typed plan/prepare boundary.

use std::fs;
use std::path::Path;
use std::process::Command as StdCommand;

use tempfile::TempDir;
use velnor_actions_contract::Plan;
use velnor_actions_orchestrator::{OrchestratorError, plan_internal, prepare, resolve_root};

use crate::impl_common::{TestResult, fixture_manifest_json, git};
use crate::impl_select::{commit, make_ws_repo, plan_pr, reasons_for};

/// Canonical origin for Velnor-policy fixtures.
const CANONICAL_ORIGIN: &str = "https://github.com/tailrocks/velnor-new.git";

/// Velnor-policy config with an explicit branch (no `origin/HEAD` lookup).
fn velnor_config() -> &'static str {
    "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n"
}

/// Skip only when an ambient non-canonical identity would fail the fixture.
fn ambient_identity_blocks() -> bool {
    std::env::var("GITHUB_REPOSITORY").is_ok_and(|hint| hint != "tailrocks/velnor-new")
}

/// Minimal git fixture with Velnor config plus the release-manifest fixture.
fn make_velnor_repo() -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), velnor_config())?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    Ok(dir)
}

/// Run git with owned arguments (for dynamic paths).
fn git_owned(args: &[String], cwd: &Path) -> TestResult {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(cwd)
        .status()?;
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
    Ok(())
}

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

/// Assert one member changed and the other stayed unproven.
fn assert_narrow(plan: &Plan, changed: &str, other: &str) {
    let hit = reasons_for(plan, changed);
    assert!(
        hit.iter().all(|r| *r == "affected_by_change"),
        "{changed}: {hit:?}"
    );
    let miss = reasons_for(plan, other);
    assert!(miss.iter().all(|r| *r == "unproven"), "{other}: {miss:?}");
}

#[test]
fn velnor_identity_ok_in_normal_repo() -> TestResult {
    if ambient_identity_blocks() {
        return Ok(());
    }
    let repo = make_velnor_repo()?;
    git(&["remote", "add", "origin", CANONICAL_ORIGIN], repo.path())?;
    prepare(repo.path())?;
    Ok(())
}

#[test]
fn velnor_identity_ok_in_linked_worktree() -> TestResult {
    if ambient_identity_blocks() {
        return Ok(());
    }
    let repo = make_velnor_repo()?;
    let root = repo.path();
    git(&["remote", "add", "origin", CANONICAL_ORIGIN], root)?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let holder = TempDir::new()?;
    let wt = holder.path().join("wt");
    let wt_arg = wt.display().to_string();
    git_owned(
        &[
            "worktree".to_owned(),
            "add".to_owned(),
            wt_arg,
            "-b".to_owned(),
            "p10wt".to_owned(),
        ],
        root,
    )?;
    assert!(wt.join(".git").is_file(), "linked worktree has a .git file");
    prepare(&wt)?;
    Ok(())
}

#[test]
fn velnor_identity_ok_via_include() -> TestResult {
    if ambient_identity_blocks() {
        return Ok(());
    }
    let repo = make_velnor_repo()?;
    let root = repo.path();
    let inc = root.join("shared.inc");
    fs::write(
        &inc,
        "[remote \"origin\"]\n\turl = https://github.com/tailrocks/velnor-new.git\n",
    )?;
    git_owned(
        &[
            "config".to_owned(),
            "include.path".to_owned(),
            inc.display().to_string(),
        ],
        root,
    )?;
    prepare(root)?;
    Ok(())
}

#[test]
fn velnor_identity_ok_via_worktree_config() -> TestResult {
    if ambient_identity_blocks() {
        return Ok(());
    }
    let repo = make_velnor_repo()?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    git(&["config", "extensions.worktreeConfig", "true"], root)?;
    let holder = TempDir::new()?;
    let wt = holder.path().join("wt");
    let wt_arg = wt.display().to_string();
    git_owned(
        &[
            "worktree".to_owned(),
            "add".to_owned(),
            wt_arg,
            "-b".to_owned(),
            "p10wt".to_owned(),
        ],
        root,
    )?;
    git_owned(
        &[
            "config".to_owned(),
            "--worktree".to_owned(),
            "remote.origin.url".to_owned(),
            CANONICAL_ORIGIN.to_owned(),
        ],
        &wt,
    )?;
    prepare(&wt)?;
    let err = prepare(root).expect_err("main checkout lacks the worktree origin");
    assert!(
        matches!(err, OrchestratorError::IdentityRejected { .. }),
        "got {err}"
    );
    Ok(())
}

#[test]
fn velnor_identity_rejects_mismatched_origin() -> TestResult {
    if ambient_identity_blocks() {
        return Ok(());
    }
    let repo = make_velnor_repo()?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/evil/other.git",
        ],
        repo.path(),
    )?;
    let err = prepare(repo.path()).expect_err("mismatched origin must fail");
    assert!(
        matches!(err, OrchestratorError::IdentityRejected { .. }),
        "got {err}"
    );
    assert!(
        err.to_string().contains("velnor_policy_requires"),
        "got {err}"
    );
    Ok(())
}

#[test]
fn velnor_identity_rejects_missing_origin() -> TestResult {
    if ambient_identity_blocks() {
        return Ok(());
    }
    let repo = make_velnor_repo()?;
    let err = prepare(repo.path()).expect_err("missing origin must fail");
    assert!(
        matches!(err, OrchestratorError::IdentityRejected { .. }),
        "got {err}"
    );
    Ok(())
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
