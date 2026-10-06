//! PR selection base-graph cases: corrupt/missing base, removed edges,
//! and merge-checkout head rules.

use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::plan_internal;

use crate::impl_common::{TestResult, git, git_line};
use crate::impl_select::{
    assert_all_changed, commit, make_ws_repo, plan_pr, plan_push, reasons_for, selects_both,
};

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
    git(&["merge", "--no-ff", "--no-commit", "feature"], root)?;
    fs::write(
        root.join("alpha/src/lib.rs"),
        "pub fn integration_only() {}\n",
    )?;
    commit(root, "merge")?;
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
    let (repo, base, feature, merge) = merge_checkout_repo()?;
    let root = repo.path();
    let (plan, _warnings) = plan_pr(root, Some(&base), &feature)?;
    assert_eq!(plan.head, merge, "plan binds actual integration candidate");
    assert!(selects_both(&plan), "universe kept: {:?}", plan.task_ids);
    assert_all_changed(&plan);
    plan.validate()?;
    Ok(())
}

#[test]
fn advanced_pr_base_rejects_stale_merge_and_accepts_fresh_candidate() -> TestResult {
    let (repo, base, feature, stale_merge) = merge_checkout_repo()?;
    let root = repo.path();
    git(&["checkout", "-b", "advanced", &base], root)?;
    fs::write(root.join("alpha/src/lib.rs"), "pub fn advanced_base() {}\n")?;
    let advanced_base = commit(root, "base advanced")?;
    git(&["checkout", "--detach", &stale_merge], root)?;
    for event in ["pull_request", "fork"] {
        let request = serde_json::json!({
            "schema": 1, "run_key": "local", "base": advanced_base,
            "head": feature, "event": event, "root": root.display().to_string(),
        });
        let error = plan_internal(&request.to_string()).expect_err("stale integration rejected");
        assert!(
            error.to_string().contains("checkout_base_mismatch"),
            "{error}"
        );
    }
    git(&["checkout", "advanced"], root)?;
    git(
        &["merge", "--no-ff", "feature", "-m", "fresh candidate"],
        root,
    )?;
    let fresh_merge = git_line(&["rev-parse", "HEAD"], root)?;
    let (plan, warnings) = plan_pr(root, Some(&advanced_base), &feature)?;
    assert_eq!(plan.head, fresh_merge);
    assert_eq!(plan.base.as_deref(), Some(advanced_base.as_str()));
    assert!(
        reasons_for(&plan, "alpha")
            .iter()
            .all(|reason| *reason == "forced_uncached")
    );
    assert!(
        reasons_for(&plan, "beta")
            .iter()
            .all(|reason| *reason == "affected_by_change")
    );
    assert!(warnings.is_empty(), "fresh exact base: {warnings:?}");
    Ok(())
}

#[test]
fn merge_checkout_without_event_base_is_rejected() -> TestResult {
    let (repo, _base, feature, _merge) = merge_checkout_repo()?;
    let error = plan_pr(repo.path(), None, &feature).expect_err("base binding required");
    assert!(
        error.to_string().contains("checkout_base_mismatch"),
        "{error}"
    );
    Ok(())
}

#[test]
fn divergent_push_compares_exact_before_and_after_trees() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    commit(root, "common")?;
    git(&["branch", "replacement"], root)?;
    fs::write(root.join("alpha/src/lib.rs"), "pub fn before_only() {}\n")?;
    let base = commit(root, "before")?;
    git(&["checkout", "replacement"], root)?;
    fs::write(root.join("beta/src/lib.rs"), "pub fn after_only() {}\n")?;
    let head = commit(root, "after")?;
    let (plan, warnings) = plan_push(root, Some(&base), &head)?;
    assert_all_changed(&plan);
    assert!(warnings.is_empty(), "exact tree comparison: {warnings:?}");
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
