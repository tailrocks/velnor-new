// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::run::scratch;

const BOLD: &str = "\u{1b}[1m";
const RESET: &str = "\u{1b}[0m";

fn fake_nextest(directory: &Path, script: &str) {
    let program = directory.join("cargo-nextest");
    fs::write(&program, format!("#!/bin/sh\n{script}\n")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn run_with_output(name: &str, script: &str, command: &[&str]) -> (Output, String, PathBuf) {
    let temp = scratch(name);
    fake_nextest(&temp, script);
    let outputs = temp.join("github-output");
    fs::write(&outputs, "existing=1\n").unwrap_or_else(|e| unreachable!("{e}"));
    let path = format!(
        "{}:{}",
        temp.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("run")
        .arg("--")
        .args(command)
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", &outputs)
        .env("PATH", path)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let written = fs::read_to_string(&outputs).unwrap_or_default();
    (output, written, temp)
}

const NEXTEST_RUN: [&str; 6] = [
    "cargo-nextest",
    "nextest",
    "run",
    "--archive-file",
    "a.tar.zst",
    "--no-tests=pass",
];

#[test]
fn run_appends_the_number_of_tests_nextest_ran_to_the_step_output() {
    let split = format!(
        "{{ printf '     Summary [   0.020s] {BOLD}7'; sleep 0.3; printf '{RESET} tests run: 7 passed, 0 skipped\\n'; }} >&2"
    );
    let (output, written, _) = run_with_output("tests-run-split", &split, &NEXTEST_RUN);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "existing=1\ntests_run=7\n");
}

#[test]
fn run_counts_failed_tests_and_keeps_the_exit_code_of_nextest() {
    let script = format!(
        "echo '     Summary [   1.200s] {BOLD}5{RESET} tests run: 3 passed, {BOLD}2{RESET} failed, 0 skipped' >&2; exit 100"
    );
    let (output, written, _) = run_with_output("tests-run-failed", &script, &NEXTEST_RUN);
    assert_eq!(output.status.code(), Some(100));
    assert_eq!(written, "existing=1\ntests_run=5\n");
}

#[test]
fn run_reports_a_run_without_tests_as_zero() {
    let script = "echo '     Summary [   0.001s] 0 tests run: 0 passed, 0 skipped' >&2";
    let (output, written, _) = run_with_output("tests-run-zero", script, &NEXTEST_RUN);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "existing=1\ntests_run=0\n");
}

#[test]
fn run_writes_no_count_for_other_commands_or_a_run_that_printed_no_summary() {
    let summary = "echo '     Summary [   0.020s] 9 tests run: 9 passed, 0 skipped' >&2";
    let cases: [(&str, &str, Vec<&str>); 4] = [
        ("list", summary, vec!["cargo-nextest", "nextest", "list"]),
        (
            "other",
            summary,
            vec![
                "sh",
                "-c",
                "echo '     Summary [   0.020s] 9 tests run: 9 passed'",
            ],
        ),
        (
            "crash",
            "echo 'error: no archive' >&2; exit 1",
            NEXTEST_RUN.to_vec(),
        ),
        ("other-program", summary, vec!["echo", "nextest", "run"]),
    ];
    for (name, script, command) in cases {
        let (_, written, _) = run_with_output(&format!("tests-run-no-{name}"), script, &command);
        assert_eq!(written, "existing=1\n", "{name}");
    }
}

#[test]
fn run_does_not_need_a_step_output_file() {
    let temp = scratch("tests-run-no-output");
    fake_nextest(
        &temp,
        "echo '     Summary [ 0.1s] 2 tests run: 2 passed' >&2",
    );
    let path = format!(
        "{}:{}",
        temp.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("run")
        .arg("--")
        .args(NEXTEST_RUN)
        .env("RUNNER_TEMP", &temp)
        .env_remove("GITHUB_OUTPUT")
        .env("PATH", path)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
}
