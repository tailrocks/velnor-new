//! Fmt-scope partition: per-package groups own member manifests, the
//! workspace group exists only when no per-package Fmt group does
//! (a workspace `fmt --all` would re-check member files, forbidden by
//! P05-5/R28), and derived task IDs are always unique.

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
        .proposals
        .iter()
        .map(|task| task.task_id.as_str())
        .collect();
    ids.sort_unstable();
    let mut deduped = ids.clone();
    deduped.dedup();
    assert_eq!(ids, deduped, "duplicate task ids");
    Ok(prep
        .discovery
        .proposals
        .iter()
        .filter(|task| task.task_id.contains("/fmt/"))
        .count())
}

#[test]
fn root_package_fmt_has_unique_task_ids() -> TestResult {
    let dir = make_repo(&two_config())?;
    fs::write(dir.path().join("rustfmt.toml"), "")?;
    assert_eq!(assert_unique_ids(dir.path())?, 2);
    let prep = prepare(dir.path())?;
    for task in prep
        .discovery
        .proposals
        .iter()
        .filter(|task| task.task_id.contains("/fmt/"))
    {
        assert!(
            !task.identity.unit_id.is_empty(),
            "root package keeps its per-package fmt group: {}",
            task.task_id
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
        2,
        "members only: workspace fmt --all would re-check member files (R28)"
    );
    let prep = prepare(dir.path())?;
    assert!(
        prep.discovery
            .proposals
            .iter()
            .filter(|task| task.task_id.contains("/fmt/"))
            .all(|task| !task.identity.unit_id.is_empty()),
        "no overlapping workspace fmt group alongside per-package groups"
    );
    git(&["add", "."], dir.path())?;
    git(&["commit", "-m", "one"], dir.path())?;
    let head = git_line(&["rev-parse", "HEAD"], dir.path())?;
    let plan = plan_value(dir.path(), "local", None, &head, None)?;
    assert_eq!(plan["schema"], 1);
    Ok(())
}

#[test]
fn consumer_repo_without_rustfmt_toml_emits_format_step_and_fails_on_unformatted_rust() -> TestResult
{
    use super::impl_common::passing_reports;
    use super::impl_orch_core::{merge, merge_request, push_request, set_task, success_jobs};
    use velnor_actions_contract::{FinalStatus, MatrixStatus, Plan, TaskStatus};
    use velnor_actions_orchestrator::{plan_internal, render_staged_tree};
    use velnor_actions_workflow_renderer::WORKFLOW_PATH;

    let dir = make_repo(config_with_branch())?;
    assert!(!dir.path().join("rustfmt.toml").exists());
    assert!(!dir.path().join(".rustfmt.toml").exists());
    // Write a formatting-only defect in src/lib.rs.
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn   bad_fmt( ) -> u32 { 1 }\n",
    )?;
    let prep = prepare(dir.path())?;
    assert!(
        prep.discovery
            .proposals
            .iter()
            .any(|task| task.task_id == "stack/rust/root/fmt/default"),
        "single-crate repo without rustfmt.toml must derive stack/rust/root/fmt/default"
    );
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing ci.yml")?;
    let task_manifest = tree
        .get(".github/velnor/task-execution-manifest-v1.json")
        .ok_or("missing task-execution-manifest-v1.json")?;
    assert!(
        yaml.contains("- name: Format")
            && task_manifest.contains("\"fmt\"")
            && task_manifest.contains("\"--check\""),
        "generated workflow and manifest must include Format step with cargo fmt --check:\n{yaml}\n{task_manifest}"
    );
    let fmt_out = std::process::Command::new("cargo")
        .args(["fmt", "--check", "--manifest-path"])
        .arg(dir.path().join("Cargo.toml"))
        .output()?;
    assert!(
        !fmt_out.status.success(),
        "cargo fmt --check must fail on formatting-only defect"
    );
    git(&["add", "."], dir.path())?;
    git(&["commit", "-m", "unformatted"], dir.path())?;
    let head = git_line(&["rev-parse", "HEAD"], dir.path())?;
    let response = plan_internal(&push_request(dir.path(), &head).to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    let mut reports = passing_reports(&plan)?;
    let fmt_report = reports
        .iter_mut()
        .find(|r| r.expected_task_ids.iter().any(|id| id.contains("/fmt/")))
        .ok_or("missing fmt task report in plan")?;
    set_task(fmt_report, TaskStatus::Failed, MatrixStatus::Failed)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let request = merge_request(
        &serde_json::to_value(&plan)?,
        &matrix,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let final_report = merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::Failed);
    Ok(())
}
