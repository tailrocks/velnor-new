//! PR selection cases: dual-graph fetch, broaden triggers, reason tags.

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::Plan;
use velnor_actions_orchestrator::plan_internal;

use crate::impl_common::{TestResult, config_with_branch, fixture_manifest_json, git, git_line};

/// Two-member workspace fixture; root package included when asked.
fn make_ws_repo(root_package: bool) -> Result<TempDir, Box<dyn std::error::Error>> {
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
    let header = if root_package {
        "[package]\nname = \"rootpkg\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n"
    } else {
        ""
    };
    fs::write(
        root.join("Cargo.toml"),
        format!("{header}[workspace]\nmembers = [\"alpha\", \"beta\"]\n"),
    )?;
    if root_package {
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    }
    for member in ["alpha", "beta"] {
        let dir = root.join(member);
        fs::create_dir_all(dir.join("src"))?;
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{member}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )?;
        fs::write(dir.join("src/lib.rs"), "pub fn f() {}\n")?;
    }
    Ok(dir)
}

/// Commit everything and return the new HEAD.
fn commit(root: &Path, message: &str) -> Result<String, Box<dyn std::error::Error>> {
    git(&["add", "."], root)?;
    git(&["commit", "-m", message], root)?;
    git_line(&["rev-parse", "HEAD"], root)
}

/// Regenerate a real lockfile offline with system cargo.
fn lockfile(root: &Path) -> TestResult {
    let status = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(root)
        .status()?;
    assert!(status.success(), "generate-lockfile failed");
    Ok(())
}

/// Plan base..head as a pull request; return plan plus warnings.
fn plan_pr(
    root: &Path,
    base: Option<&str>,
    head: &str,
) -> Result<(Plan, Vec<String>), Box<dyn std::error::Error>> {
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    let warnings: Vec<String> = serde_json::from_value(value["plan"]["warnings"].clone())?;
    Ok((plan, warnings))
}

/// True when both members have selected tasks.
fn selects_both(plan: &Plan) -> bool {
    plan.task_ids.iter().any(|id| id.contains("alpha"))
        && plan.task_ids.iter().any(|id| id.contains("beta"))
}

#[test]
fn narrow_change_selects_owner_only() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(
        !plan.task_ids.iter().any(|id| id.contains("alpha")),
        "alpha excluded: {:?}",
        plan.task_ids
    );
    assert!(
        plan.task_ids.iter().any(|id| id.contains("beta")),
        "beta kept: {:?}",
        plan.task_ids
    );
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn cargo_lock_change_selects_all() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    lockfile(root)?;
    let base = commit(root, "one")?;
    fs::write(
        root.join("alpha/Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.2.0\"\nedition = \"2021\"\n",
    )?;
    lockfile(root)?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(selects_both(&plan), "lock broadens: {:?}", plan.task_ids);
    assert!(
        warnings
            .iter()
            .any(|warning| warning == "cargo_lock_changed:selecting_all"),
        "tag: {warnings:?}"
    );
    Ok(())
}

#[test]
fn root_config_change_selects_all() -> TestResult {
    let repo = make_ws_repo(true)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    let manifest = fs::read_to_string(root.join("Cargo.toml"))?;
    fs::write(root.join("Cargo.toml"), format!("{manifest}# tuned\n"))?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(
        selects_both(&plan),
        "root config broadens: {:?}",
        plan.task_ids
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning == "root_config_changed:selecting_all"),
        "tag: {warnings:?}"
    );
    Ok(())
}

#[test]
fn unclassified_file_still_selects_all() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    fs::write(root.join("README.md"), "# demo\n")?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(
        selects_both(&plan),
        "unclassified broadens: {:?}",
        plan.task_ids
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning == "unclassified_files:selecting_all"),
        "tag: {warnings:?}"
    );
    Ok(())
}

#[test]
fn corrupt_base_manifest_tags_comparison_unavailable() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    fs::write(root.join("alpha/Cargo.toml"), "[[[broken\n")?;
    let base = commit(root, "one")?;
    fs::write(
        root.join("alpha/Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(
        selects_both(&plan),
        "missing base graph broadens: {:?}",
        plan.task_ids
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("comparison_unavailable")),
        "tag: {warnings:?}"
    );
    Ok(())
}

#[test]
fn removed_dependency_keeps_consumer_selected() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    fs::write(
        root.join("alpha/Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nbeta = { path = \"../beta\" }\n",
    )?;
    let base = commit(root, "one")?;
    fs::write(
        root.join("alpha/Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(
        selects_both(&plan),
        "removed edge keeps consumer: {:?}",
        plan.task_ids
    );
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("comparison_unavailable")),
        "base graph fetched: {warnings:?}"
    );
    Ok(())
}

#[test]
fn missing_or_bad_base_tags_comparison_unavailable() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let head = commit(root, "one")?;
    let (plan, warnings) = plan_pr(root, None, &head)?;
    assert!(selects_both(&plan), "missing base broadens");
    assert!(
        warnings
            .iter()
            .any(|warning| warning == "comparison_unavailable:missing_base:selecting_all"),
        "tag: {warnings:?}"
    );
    let (plan, warnings) = plan_pr(root, Some(&"0".repeat(40)), &head)?;
    assert!(selects_both(&plan), "bad base broadens");
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("comparison_unavailable")),
        "tag: {warnings:?}"
    );
    Ok(())
}
