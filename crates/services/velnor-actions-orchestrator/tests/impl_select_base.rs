//! PR selection base-graph cases: corrupt/missing base, removed edges,
//! and merge-checkout head rules.

use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::plan_internal;

use crate::impl_select::{
    assert_all_changed, commit, make_ws_repo, plan_pr, plan_push, reasons_for, selects_both,
};
use crate::support::{TestResult, git, git_line};

/// Repo checked out at a merge commit; returns base, feature tip, merge HEAD.
///
/// The feature tip is `HEAD^2` of the checkout: the merge result GitHub
/// checks out for pull requests.
fn merge_checkout_repo() -> Result<(TempDir, String, String, String), Box<dyn std::error::Error>> {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    git(&["checkout", "-b", "feature"], root)?;
    fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let feature = commit(root, "feat")?;
    git(&["checkout", "testmain"], root)?;
    git(&["merge", "--no-ff", "feature", "-m", "merge"], root)?;
    let merge_head = git_line(&["rev-parse", "HEAD"], root)?;
    assert_eq!(git_line(&["rev-parse", "HEAD^2"], root)?, feature);
    Ok((repo, base, feature, merge_head))
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
    let alpha = reasons_for(&plan, "alpha");
    assert!(
        alpha.iter().all(|r| *r == "affected_by_change"),
        "{alpha:?}"
    );
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("comparison_unavailable")),
        "base graph fetched: {warnings:?}"
    );
    Ok(())
}

/// Workspace where `alpha` depends on `beta` through one manifest section.
///
/// Commits the dependency as `base`, then changes only `beta` sources as
/// `head`, so `alpha` selects exclusively through reverse closure.
fn make_dep_kind_repo(
    section: &str,
) -> Result<(TempDir, String, String), Box<dyn std::error::Error>> {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    fs::write(
        root.join("alpha/Cargo.toml"),
        format!(
            "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n{section}"
        ),
    )?;
    let base = commit(root, "one")?;
    fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    Ok((repo, base, head))
}

/// Consumer reachable only through `kind` selects with an affected reason.
fn assert_kind_selects(section: &str, kind: &str) -> TestResult {
    let (repo, base, head) = make_dep_kind_repo(section)?;
    let (plan, warnings) = plan_pr(repo.path(), Some(&base), &head)?;
    assert!(selects_both(&plan), "{kind}: universe kept");
    let alpha = reasons_for(&plan, "alpha");
    assert!(
        alpha.iter().all(|r| *r == "affected_by_change"),
        "{kind}: {alpha:?}"
    );
    assert!(warnings.is_empty(), "{kind}: narrow, got {warnings:?}");
    Ok(())
}

#[test]
fn build_only_edge_selects_consumer() -> TestResult {
    assert_kind_selects(
        "[build-dependencies]\nbeta = { path = \"../beta\" }\n",
        "build",
    )
}

#[test]
fn dev_only_edge_selects_consumer() -> TestResult {
    assert_kind_selects("[dev-dependencies]\nbeta = { path = \"../beta\" }\n", "dev")
}

#[test]
fn optional_edge_selects_consumer() -> TestResult {
    assert_kind_selects(
        "[dependencies]\nbeta = { path = \"../beta\", optional = true }\n",
        "optional",
    )
}

#[test]
fn target_cfg_edge_selects_consumer() -> TestResult {
    assert_kind_selects(
        "[target.'cfg(windows)'.dependencies]\nbeta = { path = \"../beta\" }\n",
        "target-cfg",
    )
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
            .any(|warning| warning == "comparison_unavailable:missing_base:all_changed"),
        "tag: {warnings:?}"
    );
    assert_all_changed(&plan);
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

#[test]
fn push_missing_base_tags_comparison_unavailable() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let head = commit(root, "one")?;
    let (plan, warnings) = plan_push(root, None, &head)?;
    assert!(selects_both(&plan), "missing base broadens");
    assert!(
        warnings
            .iter()
            .any(|warning| warning == "comparison_unavailable:missing_base:all_changed"),
        "tag: {warnings:?}"
    );
    assert_all_changed(&plan);
    Ok(())
}

#[test]
fn merge_checkout_accepts_head2_for_pr() -> TestResult {
    let (repo, base, feature, _merge) = merge_checkout_repo()?;
    let root = repo.path();
    let (plan, _warnings) = plan_pr(root, Some(&base), &feature)?;
    assert_eq!(plan.head, feature, "planned the merge parent");
    assert!(selects_both(&plan), "universe kept: {:?}", plan.task_ids);
    plan.validate()?;
    Ok(())
}

#[test]
fn merge_checkout_rejects_head2_for_push() -> TestResult {
    let (repo, _base, feature, merge_head) = merge_checkout_repo()?;
    let root = repo.path();
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": None::<String>,
        "head": feature,
        "event": "push",
        "root": root.display().to_string(),
    });
    let err = plan_internal(&request.to_string()).expect_err("push rejects merge checkout");
    assert!(err.to_string().contains("checkout_head_mismatch"), "{err}");
    // The merge commit itself plans normally under push.
    let (plan, _warnings) = plan_push(root, None, &merge_head)?;
    assert_eq!(plan.head, merge_head);
    Ok(())
}
