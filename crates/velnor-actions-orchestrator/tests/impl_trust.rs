//! Trust cases: rev validation, source-build identity, hints, event scope.

use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::Plan;
use velnor_actions_orchestrator::plan_internal;

use crate::impl_common::{
    TestResult, config_with_branch, fixture_manifest_json, git, git_line, make_repo,
};

/// Two-commit single-package repo plus base and head SHAs.
fn two_commit_repo() -> Result<(TempDir, String, String), Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    commit(root, "one")?;
    std::fs::write(root.join("src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
    let head = commit(root, "two")?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    Ok((repo, base, head))
}

/// Two-member workspace fixture, uncommitted; tests commit as needed.
fn two_pkg_repo() -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    std::fs::create_dir_all(root.join(".velnor"))?;
    std::fs::write(root.join(".velnor/config.toml"), config_with_branch())?;
    std::fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"alpha\", \"beta\"]\n",
    )?;
    for member in ["alpha", "beta"] {
        let member_dir = root.join(member);
        std::fs::create_dir_all(member_dir.join("src"))?;
        let manifest =
            format!("[package]\nname = \"{member}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
        std::fs::write(member_dir.join("Cargo.toml"), manifest)?;
        std::fs::write(member_dir.join("src/lib.rs"), "pub fn f() {}\n")?;
    }
    Ok(dir)
}

/// Commit everything; returns the new HEAD SHA.
fn commit(root: &Path, message: &str) -> Result<String, Box<dyn std::error::Error>> {
    git(&["add", "."], root)?;
    git(&["commit", "-m", message], root)?;
    git_line(&["rev-parse", "HEAD"], root)
}

/// Plan as a pull request; plan plus warnings.
fn plan_with(
    root: &Path,
    base: Option<&str>,
    head: &str,
) -> Result<(Plan, Vec<String>), Box<dyn std::error::Error>> {
    let request = serde_json::json!({
        "schema": 1, "run_key": "local", "base": base, "head": head,
        "event": "pull_request", "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    let warnings: Vec<String> = serde_json::from_value(value["plan"]["warnings"].clone())?;
    Ok((plan, warnings))
}

#[test]
fn injection_revs_broaden_without_spawning_flags() -> TestResult {
    let repo = two_pkg_repo()?;
    let root = repo.path();
    let base = commit(root, "one")?;
    std::fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    for evil in [
        "--output=/tmp/velnor_trust_pwn",
        "$(touch /tmp/velnor_trust_pwn)",
        "-x",
    ] {
        let (plan, warnings) = plan_with(root, Some(evil), &head)?;
        assert!(
            plan.task_ids.iter().any(|id| id.contains("alpha")),
            "broadens: {:?}",
            plan.task_ids
        );
        assert!(
            plan.task_ids.iter().any(|id| id.contains("beta")),
            "broadens: {:?}",
            plan.task_ids
        );
        assert!(
            warnings.iter().any(|w| w.contains("bad_base")),
            "{warnings:?}"
        );
        let err = plan_with(root, Some(&base), evil).expect_err("evil head rejected");
        assert!(err.to_string().contains("bad_head"), "got {err}");
        assert!(
            !std::path::Path::new("/tmp/velnor_trust_pwn").exists(),
            "no flag effect"
        );
    }
    Ok(())
}

#[test]
fn short_sha_base_still_classifies() -> TestResult {
    let repo = two_pkg_repo()?;
    let root = repo.path();
    let base = commit(root, "one")?;
    std::fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_with(root, Some(&base[..7]), &head)?;
    for member in ["alpha", "beta"] {
        let obs: Vec<_> = plan
            .obligations
            .iter()
            .filter(|ob| ob.task_id.contains(member))
            .collect();
        let want = if member == "beta" {
            "affected_by_change"
        } else {
            "not_affected"
        };
        assert!(
            !obs.is_empty() && obs.iter().all(|ob| ob.reason == want),
            "{member}: {obs:?}"
        );
    }
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn new_package_at_head_marks_affected() -> TestResult {
    let repo = two_pkg_repo()?;
    let root = repo.path();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"alpha\", \"beta\", \"gamma\"]\n",
    )?;
    let base = commit(root, "one")?;
    let gamma = root.join("gamma");
    std::fs::create_dir_all(gamma.join("src"))?;
    std::fs::write(
        gamma.join("Cargo.toml"),
        "[package]\nname = \"gamma\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    std::fs::write(gamma.join("src/lib.rs"), "pub fn f() {}\n")?;
    std::fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_with(root, Some(&base), &head)?;
    for member in ["alpha", "beta", "gamma"] {
        let obs: Vec<_> = plan
            .obligations
            .iter()
            .filter(|ob| ob.task_id.contains(member))
            .collect();
        let want = if member == "alpha" {
            "not_affected"
        } else {
            "affected_by_change"
        };
        assert!(
            !obs.is_empty() && obs.iter().all(|ob| ob.reason == want),
            "{member}: {obs:?}"
        );
    }
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("comparison_unavailable")),
        "added skipped: {warnings:?}"
    );
    Ok(())
}

#[test]
fn plan_rejects_claimed_generator_identity() -> TestResult {
    use crate::impl_common::err_of;
    let (repo, base, head) = two_commit_repo()?;
    // A hand-written request claiming a release pin fails: the plan
    // always names the running binary, so no request field can bless a
    // source build as a release.
    let request = serde_json::json!({
        "schema": 1, "run_key": "local", "base": Some(&base), "head": head,
        "event": "pull_request", "root": repo.path().display().to_string(),
        "generator": {
            "version": env!("CARGO_PKG_VERSION"),
            "target": "x86_64-unknown-linux-gnu",
            "sha256": "e".repeat(64),
        },
    });
    let err = err_of(plan_internal(&request.to_string()), "claimed generator")?;
    assert!(
        err.to_string().contains("malformed_request"),
        "hand-written release pin rejected: {err}"
    );
    Ok(())
}

#[test]
fn release_lock_never_resolves_generator_identity() -> TestResult {
    let (repo, base, head) = two_commit_repo()?;
    let sha = "e".repeat(64);
    let version = env!("CARGO_PKG_VERSION");
    let mut bins = String::new();
    for target in velnor_actions_contract::SUPPORTED_TARGETS {
        use std::fmt::Write as _;
        write!(bins, "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://example.invalid/r/{version}/{target}\"\nsha256 = \"{sha}\"\n").expect("write to String");
    }
    let lock = format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{version}\"\ncommit = \"{}\"\n{bins}[mise-bootstrap]\nversion = \"{mise_version}\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n[[actions]]\nname = \"actions/checkout\"\nversion = \"v7.0.1\"\nsha = \"{}\"\nreviewed = \"2026-09-28\"\n",
        "f".repeat(40),
        "c".repeat(64),
        "d".repeat(40),
        mise_version = velnor_actions_mise::MISE_VERSION,
    );
    std::fs::write(repo.path().join(".velnor/generator.lock"), lock)?;
    let (plan, _) = plan_with(repo.path(), Some(&base), &head)?;
    // The running binary names itself: the lock pin is never adopted,
    // so a source build can never emit a release-pinned identity.
    assert_ne!(plan.generator.sha256, sha, "lock pin never adopted");
    assert_eq!(plan.generator.sha256.len(), 64);
    assert!(
        plan.generator.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
        "observed exe SHA is real hex: {}",
        plan.generator.sha256
    );
    Ok(())
}

#[test]
fn merge_group_classifies_like_pull_request() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    std::fs::write(root.join("src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let plan_for = |event: &str| -> Result<Plan, Box<dyn std::error::Error>> {
        let request = serde_json::json!({
            "schema": 1,
            "run_key": "local",
            "base": base,
            "head": head,
            "event": event,
            "root": root.display().to_string(),
        });
        let response = plan_internal(&request.to_string())?;
        let value: serde_json::Value = serde_json::from_str(&response)?;
        Ok(serde_json::from_value(value["plan"].clone())?)
    };
    let pr = plan_for("pull_request")?;
    let group = plan_for("merge_group")?;
    assert_eq!(pr.task_ids, group.task_ids);
    assert!(
        !group.task_ids.is_empty(),
        "merge group selects affected work"
    );
    assert_eq!(pr.trust, velnor_actions_contract::Trust::Pr);
    assert_eq!(
        group.trust,
        velnor_actions_contract::Trust::Pr,
        "speculative merge content stays PR-scoped"
    );
    let push = plan_for("push")?;
    assert_eq!(push.trust, velnor_actions_contract::Trust::Trusted);
    Ok(())
}
