// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use crate::support::fresh_dir;

const SPEC: &str =
    "--plan rust-plan --job rust-plan=always --job rust-test=lane:root --tests rust-test=root";

fn canonical(text: &str) -> bool {
    !(text.is_empty() || text.len() > 1 && text.starts_with('0'))
        && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn needs(plan_result: &str, test_result: &str, tests_run: &str, listed: &str) -> String {
    let packages = if canonical(tests_run) && canonical(listed) {
        format!(r#","tests_by_package":"pkg={tests_run}/{listed}""#)
    } else {
        String::new()
    };
    format!(
        r#"{{"rust-plan":{{"result":"{plan_result}","outputs":{{"rust_root_mode":"some","rust_root_tests":"run"}}}},"rust-test":{{"result":"{test_result}","outputs":{{"tests_run":"{tests_run}","tests_listed":"{listed}"{packages}}}}}}}"#
    )
}

fn gate(label: &str, needs: Option<&str>) -> (Output, String) {
    gate_with(label, SPEC, needs)
}

fn gate_with(label: &str, spec: &str, needs: Option<&str>) -> (Output, String) {
    let dir = fresh_dir("velnor-ci-gate", label);
    let summary = dir.join("summary");
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(["gate", "--spec-env", "GATE_SPEC"])
        .env("GATE_SPEC", spec)
        .env_remove("NEEDS_JSON")
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_STEP_SUMMARY", &summary);
    if let Some(needs) = needs {
        command.env("NEEDS_JSON", needs);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    (output, fs::read_to_string(&summary).unwrap_or_default())
}

#[test]
fn a_green_plan_with_tests_exits_0_and_writes_the_table_to_the_step_summary() {
    let (output, summary) = gate("green", Some(&needs("success", "success", "7", "7")));
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, summary);
    assert!(stdout.contains("**Required: pass**"), "{stdout}");
    assert!(
        stdout.contains("| rust-test | scheduled | success |"),
        "{stdout}"
    );
}

#[test]
fn every_fail_closed_case_exits_1_and_names_the_reason() {
    let cases = [
        (
            "plan-failed",
            needs("failure", "skipped", "", ""),
            "did not succeed",
        ),
        (
            "cancelled",
            needs("success", "cancelled", "7", "7"),
            "result is cancelled",
        ),
        (
            "skipped",
            needs("success", "skipped", "", ""),
            "result is skipped",
        ),
        (
            "zero",
            needs("success", "success", "0", "3"),
            "package pkg of root listed 3 tests and ran none",
        ),
        (
            "no-listed",
            needs("success", "success", "7", ""),
            "tests_by_package is missing or malformed",
        ),
        (
            "empty",
            needs("success", "success", "", ""),
            "tests_by_package is missing or malformed",
        ),
        (
            "not-json",
            "not json".to_owned(),
            "NEEDS_JSON is not the needs context",
        ),
        (
            "not-an-object",
            "[]".to_owned(),
            "NEEDS_JSON is not the needs context",
        ),
        (
            "empty-object",
            "{}".to_owned(),
            "the plan job rust-plan is not in needs",
        ),
    ];
    for (label, needs, reason) in cases {
        let (output, summary) = gate(label, Some(&needs));
        assert_eq!(output.status.code(), Some(1), "{label}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(reason), "{label}: {stderr}");
        assert!(summary.contains("**Required: FAIL**"), "{label}: {summary}");
    }
}

#[test]
fn a_missing_needs_variable_fails() {
    let (output, _) = gate("no-needs", None);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("NEEDS_JSON is not set"));
}

#[test]
fn a_missing_or_malformed_spec_fails_before_anything_passes() {
    for spec in [
        None,
        Some(""),
        Some("--plan rust-plan --job rust-plan=sometimes"),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        command
            .args(["gate", "--spec-env", "GATE_SPEC"])
            .env_remove("GATE_SPEC")
            .env("NEEDS_JSON", needs("success", "success", "7", "7"));
        if let Some(spec) = spec {
            command.env("GATE_SPEC", spec);
        }
        let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(output.status.code(), Some(1), "{spec:?}: {output:?}");
    }
}

fn budget_needs(result: &str) -> String {
    format!(
        r#"{{"rust-plan":{{"result":"success","outputs":{{"rust_root_mode":"some","rust_root_tests":"run"}}}},"rust-test":{{"result":"success","outputs":{{"tests_run":"7","tests_listed":"7","tests_by_package":"pkg=7/7"}}}},"rust-budget":{{"result":"{result}","outputs":{{"budget_pr_wall_seconds":"300","budget_main_wall_seconds":"600","budget_runner_seconds":"1800","budget_setup_seconds":"60","run_wall_seconds":"420","run_runner_seconds":"900"}}}}}}"#
    )
}

const BUDGET_SPEC: &str = "--plan rust-plan --job rust-plan=always --job rust-test=lane:root --tests rust-test=root --job rust-budget=advisory";

#[test]
fn the_gate_prints_the_budget_next_to_the_current_run() {
    let (output, summary) = gate_with("budget", BUDGET_SPEC, Some(&budget_needs("success")));
    assert!(output.status.success(), "{output:?}");
    assert!(summary.contains("**Speed budget**"), "{summary}");
    assert!(
        summary.contains("| Wall seconds | 420s | 300s |"),
        "{summary}"
    );
    assert!(
        summary.contains("| Runner seconds | 900s | 1800s |"),
        "{summary}"
    );
}

#[test]
fn the_gate_prints_no_budget_when_the_budget_job_did_not_run() {
    let (output, summary) = gate("no-budget", Some(&needs("success", "success", "7", "7")));
    assert!(output.status.success(), "{output:?}");
    assert!(!summary.contains("Speed budget"), "{summary}");
}

#[test]
fn a_failed_budget_job_does_not_fail_the_required_check() {
    let (output, summary) = gate_with("budget-failed", BUDGET_SPEC, Some(&budget_needs("failure")));
    assert!(output.status.success(), "{output:?}");
    assert!(
        summary
            .contains("warning: rust-budget was scheduled by the plan and its result is failure"),
        "{summary}"
    );
}
