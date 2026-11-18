// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_model::RepoView;
use velnor_rust::LOCK_PATH;

use crate::facts::TestResult;
use crate::pipeline::{HELPER_LOCK, view, workflow};

fn pinned(version: &str) -> String {
    String::from_utf8_lossy(HELPER_LOCK).replace("0.1.79", version)
}

fn budgeted(version: &str, config: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut files = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    files.files.insert(LOCK_PATH, pinned(version).into_bytes());
    Ok(files)
}

#[test]
fn a_budget_renders_a_job_that_measures_the_last_runs_of_the_workflow() -> TestResult {
    let files = budgeted(
        "0.1.82",
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
        "0.1.82",
        "full_run = \"off\"\n[budget]\nmain_wall_seconds = 600\n",
    )?;
    let text = workflow(&files)?;
    let job = text
        .split("  rust-budget:")
        .nth(1)
        .ok_or("the budget job is missing")?;
    assert!(job.contains("github.event_name == ''push''"), "{job}");
    Ok(())
}

#[test]
fn the_budget_job_runs_on_the_schedule_when_a_full_run_is_scheduled() -> TestResult {
    let files = budgeted(
        "0.1.82",
        "full_run = \"nightly\"\n[budget]\nmain_wall_seconds = 600\n",
    )?;
    let text = workflow(&files)?;
    let job = text
        .split("  rust-budget:")
        .nth(1)
        .ok_or("the budget job is missing")?;
    assert!(job.contains("github.event_name == ''schedule''"), "{job}");
    Ok(())
}

#[test]
fn a_budget_of_zero_renders_no_job() -> TestResult {
    let files = budgeted(
        "0.1.82",
        "[budget]\npr_wall_seconds = 0\nmain_wall_seconds = 0\nrunner_seconds = 0\nsetup_seconds = 0\n",
    )?;
    let text = workflow(&files)?;
    assert!(!text.contains("rust-budget"), "{text}");
    Ok(())
}

#[test]
fn the_budget_job_is_advisory_in_the_required_check() -> TestResult {
    let files = budgeted("0.1.82", "[budget]\nmain_wall_seconds = 600\n")?;
    let text = workflow(&files)?;
    let required = text
        .split("  required:")
        .nth(1)
        .ok_or("the required job is missing")?;
    assert!(
        required.contains("--job rust-budget=schedule+advisory"),
        "a breach must not fail the required check\n{required}"
    );
    Ok(())
}

#[test]
fn a_helper_without_the_budget_command_is_refused() -> TestResult {
    let files = budgeted("0.1.81", "[budget]\nmain_wall_seconds = 600\n")?;
    let error = velnor_gen::expected_with_notes(&files).expect_err("the old helper was accepted");
    let GenerateError::BudgetHelperTooOld(version) = error else {
        return Err(format!("unexpected error: {error}").into());
    };
    assert_eq!(version.to_string(), "0.1.81");
    Ok(())
}

#[test]
fn an_unknown_key_in_the_budget_is_a_config_error() -> TestResult {
    let files = budgeted("0.1.82", "[budget]\nwall_seconds = 600\n")?;
    let error = velnor_gen::expected_with_notes(&files).expect_err("the unknown key was accepted");
    assert!(error.to_string().contains("budget.wall_seconds"), "{error}");
    Ok(())
}
