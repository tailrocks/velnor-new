// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use crate::support::fresh_dir;

const ARGS: [&str; 9] = [
    "gate",
    "--plan",
    "rust-plan",
    "--job",
    "rust-plan=always",
    "--job",
    "rust-test=lane:root",
    "--tests",
    "rust-test=root",
];

fn needs(plan_result: &str, test_result: &str, tests_run: &str) -> String {
    format!(
        r#"{{"rust-plan":{{"result":"{plan_result}","outputs":{{"rust_root_mode":"some","rust_root_tests":"run"}}}},"rust-test":{{"result":"{test_result}","outputs":{{"tests_run":"{tests_run}"}}}}}}"#
    )
}

fn gate(label: &str, needs: Option<&str>) -> (Output, String) {
    let dir = fresh_dir("velnor-ci-gate", label);
    let summary = dir.join("summary");
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(ARGS)
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
    let (output, summary) = gate("green", Some(&needs("success", "success", "7")));
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
            needs("failure", "skipped", ""),
            "did not succeed",
        ),
        (
            "cancelled",
            needs("success", "cancelled", "7"),
            "result is cancelled",
        ),
        (
            "skipped",
            needs("success", "skipped", ""),
            "result is skipped",
        ),
        (
            "zero",
            needs("success", "success", "0"),
            "0 tests ran for root",
        ),
        (
            "empty",
            needs("success", "success", ""),
            "missing or not a number",
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
fn malformed_gate_arguments_fail_before_anything_passes() {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args([
            "gate",
            "--plan",
            "rust-plan",
            "--job",
            "rust-plan=sometimes",
        ])
        .env("NEEDS_JSON", needs("success", "success", "7"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}
