//! P10 Git-authority cases: origin identity through `git config`
//! (normal repo, linked worktree, include, worktree config, reject
//! mismatch/missing) through the typed prepare boundary.

use crate::impl_common::git_fixture;

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_orchestrator::{OrchestratorError, prepare};

use crate::impl_common::{TestResult, fixture_manifest_json, git, without_ambient_identity};

/// Canonical origin for Velnor-policy fixtures.
const CANONICAL_ORIGIN: &str = "https://github.com/tailrocks/velnor-new.git";

/// Velnor-policy config with an explicit branch (no `origin/HEAD` lookup).
fn velnor_config() -> &'static str {
    "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n"
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
    let status = git_fixture::command(cwd)?
        .args(args)
        .current_dir(cwd)
        .status()?;
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
    Ok(())
}

#[test]
fn velnor_identity_ok_in_normal_repo() -> TestResult {
    without_ambient_identity("velnor_identity_ok_in_normal_repo", || {
        let repo = make_velnor_repo()?;
        git(&["remote", "add", "origin", CANONICAL_ORIGIN], repo.path())?;
        prepare(repo.path())?;
        Ok(())
    })
}

#[test]
fn velnor_identity_ok_in_linked_worktree() -> TestResult {
    without_ambient_identity("velnor_identity_ok_in_linked_worktree", || {
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
    })
}

#[test]
fn velnor_identity_ok_via_include() -> TestResult {
    without_ambient_identity("velnor_identity_ok_via_include", || {
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
    })
}

#[test]
fn velnor_identity_ok_via_worktree_config() -> TestResult {
    without_ambient_identity("velnor_identity_ok_via_worktree_config", || {
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
    })
}

#[test]
fn velnor_identity_rejects_mismatched_origin() -> TestResult {
    without_ambient_identity("velnor_identity_rejects_mismatched_origin", || {
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
    })
}

#[test]
fn velnor_identity_rejects_missing_origin() -> TestResult {
    without_ambient_identity("velnor_identity_rejects_missing_origin", || {
        let repo = make_velnor_repo()?;
        let err = prepare(repo.path()).expect_err("missing origin must fail");
        assert!(
            matches!(err, OrchestratorError::IdentityRejected { .. }),
            "got {err}"
        );
        Ok(())
    })
}
