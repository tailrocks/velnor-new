// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_model::RepoView;
use velnor_rust::LOCK_PATH;

use crate::facts::TestResult;
use crate::pipeline::{HELPER_LOCK, view, workflow};

fn pinned(version: &str) -> String {
    String::from_utf8_lossy(HELPER_LOCK).replace("0.1.81", version)
}

fn budgeted(version: &str, config: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut files = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    files.files.insert(LOCK_PATH, pinned(version).into_bytes());
    Ok(files)
}

#[test]
fn a_budget_renders_a_job_that_measures_the_last_runs_of_the_workflow() -> TestResult {
    let files = budgeted(
        "0.1.92",
        "[budget]\npr_wall_seconds = 300\nmain_wall_seconds = 600\nrunner_seconds = 1800\nsetup_seconds = 60\n",
    )?;
    let text = workflow(&files)?;
    assert!(text.contains("  rust-budget:"), "{text}");
    assert!(text.contains("bench budget --last 10"), "{text}");
    assert!(
        text.contains("GITHUB_TOKEN: '${{ github.token }}'"),
        "{text}"
    );
    assert!(
        text.contains("actions: read"),
        "the job reads the runs of the workflow\n{text}"
    );
    Ok(())
}

#[test]
fn the_budget_job_runs_on_the_default_branch_when_no_full_run_is_scheduled() -> TestResult {
    let files = budgeted(
        "0.1.92",
        "full_run = \"off\"\n[budget]\nmain_wall_seconds = 600\n",
    )?;
    let text = workflow(&files)?;
    let job = text
        .split("  rust-budget:")
        .nth(1)
        .ok_or("the budget job is missing")?;
    assert!(
        job.contains(DEFAULT_BRANCH_PUSH),
        "the job runs on a push to the default branch and on nothing else\n{job}"
    );
    assert!(
        !job.contains("github.event_name == ''schedule''"),
        "without a full run there is no scheduled run to check\n{job}"
    );
    Ok(())
}

const DEFAULT_BRANCH_PUSH: &str = "(github.event_name == ''push'' && github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch))";

fn condition_of_budget_job(config: &str) -> Result<String, Box<dyn Error>> {
    let text = workflow(&budgeted("0.1.92", config)?)?;
    let job = text
        .split("  rust-budget:")
        .nth(1)
        .ok_or("the budget job is missing")?;
    job.lines()
        .find(|line| line.trim_start().starts_with("if:"))
        .map(str::to_owned)
        .ok_or_else(|| "the budget job has no condition".into())
}

#[test]
fn the_budget_job_runs_on_default_branch_pushes_and_on_the_schedule_when_a_full_run_is_scheduled()
-> TestResult {
    for config in [
        "full_run = \"nightly\"\n[budget]\nmain_wall_seconds = 600\n",
        "[budget]\nmain_wall_seconds = 600\n",
    ] {
        let condition = condition_of_budget_job(config)?;
        assert!(
            condition.contains(DEFAULT_BRANCH_PUSH),
            "the pushes that #622 judges per selection run the job\n{condition}"
        );
        assert!(
            condition.contains("github.event_name == ''schedule''"),
            "the nightly full run is checked as well\n{condition}"
        );
    }
    Ok(())
}

#[test]
fn the_budget_job_never_runs_on_a_pull_request_or_a_merge_group() -> TestResult {
    for config in [
        "full_run = \"off\"\n[budget]\nmain_wall_seconds = 600\n",
        "full_run = \"nightly\"\n[budget]\nmain_wall_seconds = 600\n",
    ] {
        let whole = condition_of_budget_job(config)?;
        // Every job also carries the guard that drops a closed pull request.
        let condition = whole
            .split_once("(!cancelled()")
            .ok_or("the job condition has no status function")?
            .1;
        for event in ["pull_request", "merge_group", "workflow_dispatch"] {
            assert!(
                !condition.contains(event),
                "{event} must not run the budget job\n{condition}"
            );
        }
        assert!(
            !condition
                .replace(DEFAULT_BRANCH_PUSH, "")
                .contains("''push''"),
            "a push counts only on the default branch\n{condition}"
        );
    }
    Ok(())
}

#[test]
fn the_budget_job_runs_when_an_ancestor_was_skipped_but_not_when_the_run_was_cancelled()
-> TestResult {
    for (config, event) in [
        (
            "full_run = \"off\"\n[budget]\nmain_wall_seconds = 600\n",
            "github.event_name == ''push''",
        ),
        (
            "full_run = \"nightly\"\n[budget]\nmain_wall_seconds = 600\n",
            "github.event_name == ''schedule''",
        ),
        (
            "full_run = \"nightly\"\n[budget]\nmain_wall_seconds = 600\n",
            "github.event_name == ''push''",
        ),
    ] {
        let files = budgeted("0.1.92", config)?;
        let text = workflow(&files)?;
        let job = text
            .split("  rust-budget:")
            .nth(1)
            .ok_or("the budget job is missing")?;
        let condition = job
            .lines()
            .find(|line| line.trim_start().starts_with("if:"))
            .ok_or("the budget job has no condition")?;
        assert!(
            condition.contains("!cancelled()"),
            "without a status function a skipped ancestor skips the job\n{condition}"
        );
        assert!(condition.contains(event), "{condition}");
        assert!(
            condition.contains("needs.required.result == ''success''"),
            "a failed gate is not measured\n{condition}"
        );
    }
    Ok(())
}

#[test]
fn a_budget_of_zero_renders_no_job() -> TestResult {
    let files = budgeted(
        "0.1.92",
        "[budget]\npr_wall_seconds = 0\nmain_wall_seconds = 0\nrunner_seconds = 0\nsetup_seconds = 0\n",
    )?;
    let text = workflow(&files)?;
    assert!(!text.contains("rust-budget"), "{text}");
    Ok(())
}

#[test]
fn the_budget_job_runs_after_the_required_check_and_the_check_does_not_wait_for_it() -> TestResult {
    for config in [
        "[budget]\nmain_wall_seconds = 600\n",
        "full_run = \"off\"\n[budget]\nmain_wall_seconds = 600\n",
    ] {
        let files = budgeted("0.1.92", config)?;
        let tree = crate::yaml_tree::document(&workflow(&files)?)?;
        let required = tree.pointer("/jobs/required").ok_or("no required job")?;
        assert!(
            !required.to_string().contains("rust-budget"),
            "the gate waits for the advisory scan\n{required}"
        );
        assert_eq!(
            tree.pointer("/jobs/rust-budget/needs"),
            Some(&serde_json::json!(["required"]))
        );
    }
    Ok(())
}

#[test]
fn a_helper_without_the_budget_command_is_refused() -> TestResult {
    let files = budgeted("0.1.81", "[budget]\nmain_wall_seconds = 600\n")?;
    let error = velnor_gen::expected_with_notes(&files).expect_err("the old helper was accepted");
    let GenerateError::ConfigKeyHelperTooOld {
        key,
        pinned: version,
        ..
    } = error
    else {
        return Err(format!("unexpected error: {error}").into());
    };
    assert_eq!(version.to_string(), "0.1.81");
    assert!(key.starts_with("budget"), "{key}");
    Ok(())
}

#[test]
fn a_pin_of_0_1_91_is_refused_because_it_predates_the_budget_command() -> TestResult {
    let files = budgeted("0.1.91", "[budget]\nmain_wall_seconds = 600\n")?;
    let error = velnor_gen::expected_with_notes(&files).expect_err("the 0.1.91 pin was accepted");
    let GenerateError::ConfigKeyHelperTooOld {
        key,
        pinned: version,
        ..
    } = &error
    else {
        return Err(format!("unexpected error: {error}").into());
    };
    assert_eq!(version.to_string(), "0.1.91");
    assert!(key.starts_with("budget"), "{key}");
    assert!(
        error.to_string().contains("pin velnor-ci 0.1.92 or later"),
        "{error}"
    );
    Ok(())
}

#[test]
fn an_unknown_key_in_the_budget_is_a_config_error() -> TestResult {
    let files = budgeted("0.1.92", "[budget]\nwall_seconds = 600\n")?;
    let error = velnor_gen::expected_with_notes(&files).expect_err("the unknown key was accepted");
    assert!(error.to_string().contains("budget.wall_seconds"), "{error}");
    Ok(())
}

#[test]
fn a_pin_that_reads_the_selection_titles_the_budget_job_with_what_the_plan_selected() -> TestResult
{
    let files = budgeted("0.1.104", "[budget]\nmain_wall_seconds = 600\n")?;
    let tree = crate::yaml_tree::document(&workflow(&files)?)?;
    let name = tree
        .pointer("/jobs/rust-budget/name")
        .and_then(serde_json::Value::as_str)
        .ok_or("the budget job has no title")?;
    assert!(
        name.contains("format('Speed budget ({0})'"),
        "the title records the selection\n{name}"
    );
    for (value, mode) in [("all", "full"), ("some", "affected"), ("none", "none")] {
        assert!(
            name.contains(&format!("needs.rust.outputs.rust_root_mode == '{value}'"))
                && name.contains(&format!("'{mode}'")),
            "{name}"
        );
    }
    assert_eq!(
        tree.pointer("/jobs/rust-budget/needs"),
        Some(&serde_json::json!(["required", "rust"])),
        "the job reads the plan outputs, so it needs the job that has them"
    );
    assert_eq!(
        tree.pointer("/jobs/rust/outputs/rust_root_mode"),
        Some(&serde_json::json!(
            "${{ steps.plan.outputs.rust_root_mode }}"
        ))
    );
    Ok(())
}

#[test]
fn the_selection_title_is_only_an_expression_when_the_budget_job_runs() -> TestResult {
    let files = budgeted("0.1.104", "[budget]\nmain_wall_seconds = 600\n")?;
    let tree = crate::yaml_tree::document(&workflow(&files)?)?;
    let name = tree
        .pointer("/jobs/rust-budget/name")
        .and_then(serde_json::Value::as_str)
        .ok_or("the budget job has no title")?;
    assert!(
        name.starts_with("${{ (needs.required.result == 'success'")
            && name.contains("needs.rust.result == 'success')")
            && name.ends_with("|| 'Speed budget' }}"),
        "the title falls back to the plain name when the job is skipped\n{name}"
    );
    Ok(())
}

#[test]
fn a_pin_that_cannot_read_the_selection_keeps_the_plain_title_and_needs() -> TestResult {
    let files = budgeted("0.1.103", "[budget]\nmain_wall_seconds = 600\n")?;
    let tree = crate::yaml_tree::document(&workflow(&files)?)?;
    assert_eq!(
        tree.pointer("/jobs/rust-budget/name"),
        Some(&serde_json::json!("Speed budget"))
    );
    assert_eq!(
        tree.pointer("/jobs/rust-budget/needs"),
        Some(&serde_json::json!(["required"]))
    );
    assert_eq!(tree.pointer("/jobs/rust/outputs/rust_root_mode"), None);
    Ok(())
}

#[test]
fn per_mode_limits_need_a_pin_that_applies_them() -> TestResult {
    let config = "[budget.full]\nmain_wall_seconds = 900\n";
    let old = budgeted("0.1.103", config)?;
    let error = velnor_gen::expected_with_notes(&old).expect_err("the old pin was accepted");
    let GenerateError::BudgetModesHelperTooOld(version) = &error else {
        return Err(format!("unexpected error: {error}").into());
    };
    assert_eq!(version.to_string(), "0.1.103");
    assert!(
        error.to_string().contains("pin velnor-ci 0.1.104 or later"),
        "{error}"
    );
    let files = budgeted("0.1.104", config)?;
    assert!(workflow(&files)?.contains("  rust-budget:"));
    Ok(())
}
