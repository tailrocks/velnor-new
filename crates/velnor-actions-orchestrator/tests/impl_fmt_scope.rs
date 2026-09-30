//! Fmt-scope partition: per-package groups own member manifests, the
//! workspace group exists only when no member owns the workspace
//! manifest, and derived task IDs are always unique.

use std::fs;

use velnor_actions_orchestrator::prepare;

use super::impl_common::{
    TestResult, config_with_branch, git, git_line, make_repo, make_virtual_repo,
};
use super::impl_orch_core::plan_value;

/// Two-configuration fixture config (termpane shape: `default` + `full`).
fn two_config() -> String {
    format!(
        "{}\n[[stacks.rust.configurations]]\nname = \"default\"\nfeatures = [\"default\"]\ntarget = \"host\"\n[[stacks.rust.configurations]]\nname = \"full\"\nfeatures = []\ntarget = \"host\"\n",
        config_with_branch()
    )
}

/// Assert every derived task ID is unique, returning the fmt group count.
fn assert_unique_ids(root: &std::path::Path) -> Result<usize, Box<dyn std::error::Error>> {
    let prep = prepare(root)?;
    let mut ids: Vec<&str> = prep
        .discovery
        .task_groups
        .iter()
        .map(|group| group.task_id.as_str())
        .collect();
    ids.sort_unstable();
    let mut deduped = ids.clone();
    deduped.dedup();
    assert_eq!(ids, deduped, "duplicate task ids");
    Ok(prep
        .discovery
        .task_groups
        .iter()
        .filter(|group| group.task_id.contains("/fmt/"))
        .count())
}

#[test]
fn root_package_fmt_has_unique_task_ids() -> TestResult {
    let dir = make_repo(&two_config())?;
    fs::write(dir.path().join("rustfmt.toml"), "")?;
    assert_eq!(assert_unique_ids(dir.path())?, 2);
    let prep = prepare(dir.path())?;
    for group in prep
        .discovery
        .task_groups
        .iter()
        .filter(|group| group.task_id.contains("/fmt/"))
    {
        assert!(
            !group.package_id.is_empty(),
            "root package keeps its per-package fmt group: {}",
            group.task_id
        );
    }
    git(&["add", "."], dir.path())?;
    git(&["commit", "-m", "one"], dir.path())?;
    let head = git_line(&["rev-parse", "HEAD"], dir.path())?;
    let plan = plan_value(dir.path(), "local", None, &head, None)?;
    assert_eq!(plan["schema"], 1);
    Ok(())
}

#[test]
fn virtual_workspace_fmt_has_unique_task_ids() -> TestResult {
    let dir = make_virtual_repo(&two_config())?;
    fs::write(dir.path().join("rustfmt.toml"), "")?;
    assert_eq!(
        assert_unique_ids(dir.path())?,
        4,
        "member plus workspace fmt per configuration"
    );
    let prep = prepare(dir.path())?;
    assert!(
        prep.discovery
            .task_groups
            .iter()
            .filter(|group| group.task_id.contains("/fmt/"))
            .any(|group| group.package_id.is_empty()),
        "virtual workspace keeps its workspace fmt group"
    );
    git(&["add", "."], dir.path())?;
    git(&["commit", "-m", "one"], dir.path())?;
    let head = git_line(&["rev-parse", "HEAD"], dir.path())?;
    let plan = plan_value(dir.path(), "local", None, &head, None)?;
    assert_eq!(plan["schema"], 1);
    Ok(())
}
