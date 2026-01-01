//! PR selection cases: dual-graph fetch, broaden triggers, reason tags.

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::{ObligationDecision, Plan};
use velnor_actions_orchestrator::plan_internal;

use crate::impl_common::{TestResult, config_with_branch, fixture_manifest_json, git, git_line};

/// Init a git repo with Velnor config plus release fixture.
fn scaffold(root: &Path) -> TestResult {
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
    Ok(())
}

/// Two-member workspace fixture; root package included when asked.
pub(crate) fn make_ws_repo(root_package: bool) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    scaffold(root)?;
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

/// Workspace with a lib member `alpha` and a bin-only member `beta`.
fn make_mixed_repo() -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    scaffold(root)?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"alpha\", \"beta\"]\n",
    )?;
    for member in ["alpha", "beta"] {
        let dir = root.join(member);
        fs::create_dir_all(dir.join("src"))?;
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{member}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )?;
    }
    fs::write(root.join("alpha/src/lib.rs"), "pub fn f() {}\n")?;
    fs::write(root.join("beta/src/main.rs"), "fn main() {}\n")?;
    Ok(dir)
}

/// Commit everything and return the new HEAD.
pub(crate) fn commit(root: &Path, message: &str) -> Result<String, Box<dyn std::error::Error>> {
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
pub(crate) fn plan_pr(
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

/// Plan base..head as a push; return plan plus warnings.
pub(crate) fn plan_push(
    root: &Path,
    base: Option<&str>,
    head: &str,
) -> Result<(Plan, Vec<String>), Box<dyn std::error::Error>> {
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": head,
        "event": "push",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    let warnings: Vec<String> = serde_json::from_value(value["plan"]["warnings"].clone())?;
    Ok((plan, warnings))
}

/// True when both members have selected tasks.
pub(crate) fn selects_both(plan: &Plan) -> bool {
    plan.task_ids.iter().any(|id| id.contains("alpha"))
        && plan.task_ids.iter().any(|id| id.contains("beta"))
}

/// Reasons of one member's obligations, non-empty by construction.
pub(crate) fn reasons_for<'a>(plan: &'a Plan, member: &str) -> Vec<&'a str> {
    let reasons: Vec<&str> = plan
        .obligations
        .iter()
        .filter(|ob| ob.task_id.contains(member))
        .map(|ob| ob.reason.as_str())
        .collect();
    assert!(!reasons.is_empty(), "{member} has obligations");
    reasons
}

/// Assert every obligation executes as changed.
pub(crate) fn assert_all_changed(plan: &Plan) {
    let changed = plan
        .obligations
        .iter()
        .all(|ob| ob.reason == "affected_by_change");
    assert!(changed, "{:?}", plan.obligations);
}

#[test]
fn narrow_change_keeps_universe_with_reasons() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(selects_both(&plan), "universe kept: {:?}", plan.task_ids);
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute),
        "no proof, all execute"
    );
    let beta = reasons_for(&plan, "beta");
    assert!(beta.iter().all(|r| *r == "affected_by_change"), "{beta:?}");
    let alpha = reasons_for(&plan, "alpha");
    assert!(alpha.iter().all(|r| *r == "forced_uncached"), "{alpha:?}");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn md_in_package_selects_narrowly() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    fs::create_dir_all(root.join("alpha/docs"))?;
    fs::write(root.join("alpha/docs/notes.md"), "# notes\n")?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(selects_both(&plan), "universe kept: {:?}", plan.task_ids);
    let alpha = reasons_for(&plan, "alpha");
    assert!(
        alpha.iter().all(|r| *r == "affected_by_change"),
        "{alpha:?}"
    );
    let beta = reasons_for(&plan, "beta");
    assert!(beta.iter().all(|r| *r == "forced_uncached"), "{beta:?}");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn bin_only_package_omits_doctest_with_reason() -> TestResult {
    let repo = make_mixed_repo()?;
    let root = repo.path();
    let base = commit(root, "one")?;
    fs::write(root.join("beta/src/main.rs"), "fn main() { println!(); }\n")?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(
        plan.task_ids.iter().any(|id| id.contains("beta")),
        "beta kept: {:?}",
        plan.task_ids
    );
    assert!(
        !plan
            .task_ids
            .iter()
            .any(|id| id.contains("beta") && id.contains("doctest")),
        "no beta doctest leg: {:?}",
        plan.task_ids
    );
    assert!(
        plan.task_ids
            .iter()
            .any(|id| id.contains("beta") && id.contains("/doc/")),
        "beta doc leg kept: {:?}",
        plan.task_ids
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.starts_with("valid_no_test_targets:")
                && warning.contains("beta")
                && warning.contains("doctest")),
        "explicit omission reason: {warnings:?}"
    );
    assert!(
        !plan
            .obligations
            .iter()
            .any(|ob| ob.task_id.contains("beta") && ob.task_id.contains("doctest")),
        "no beta doctest obligation"
    );
    let (plan, warnings) = {
        fs::write(
            root.join("alpha/src/lib.rs"),
            "pub fn f() {}\npub fn g() {}\n",
        )?;
        let head = commit(root, "three")?;
        plan_pr(root, Some(&base), &head)?
    };
    assert!(
        plan.task_ids
            .iter()
            .any(|id| id.contains("alpha") && id.contains("doctest")),
        "lib doctest leg kept: {:?}",
        plan.task_ids
    );
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("alpha") && warning.contains("doctest")),
        "no omission for lib doctest: {warnings:?}"
    );
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
            .any(|warning| warning == "cargo_lock_changed:all_changed"),
        "tag: {warnings:?}"
    );
    assert_all_changed(&plan);
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
            .any(|warning| warning == "root_config_changed:all_changed"),
        "tag: {warnings:?}"
    );
    assert_all_changed(&plan);
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
            .any(|warning| warning == "unclassified_files:all_changed"),
        "tag: {warnings:?}"
    );
    assert_all_changed(&plan);
    Ok(())
}

#[test]
fn root_package_does_not_mask_unclassified_files() -> TestResult {
    let repo = make_ws_repo(true)?;
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
            .any(|warning| warning == "unclassified_files:all_changed"),
        "root must not classify stray files: {warnings:?}"
    );
    assert_all_changed(&plan);
    Ok(())
}

#[test]
fn nested_change_stays_narrow_with_root_package() -> TestResult {
    let repo = make_ws_repo(true)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(selects_both(&plan), "universe kept: {:?}", plan.task_ids);
    let beta = reasons_for(&plan, "beta");
    assert!(beta.iter().all(|r| *r == "affected_by_change"), "{beta:?}");
    let alpha = reasons_for(&plan, "alpha");
    assert!(alpha.iter().all(|r| *r == "forced_uncached"), "{alpha:?}");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}
