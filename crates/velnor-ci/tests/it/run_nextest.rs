// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use crate::run::scratch;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
const BOLD: &str = "\u{1b}[1m";
const RESET: &str = "\u{1b}[0m";
pub(crate) fn fake_nextest(directory: &Path, run: &str, list_json: &str) {
    assert!(!list_json.contains('\''), "{list_json}");
    let program = directory.join("cargo-nextest");
    let mut script = String::from("#!/bin/sh\nif [ \"$2\" = list ]; then\n");
    script.push_str("printf '%s\\n' \"$@\" > \"$RUNNER_TEMP/list-argv\"\n");
    script.push_str("echo 'Summary [   0.001s] 99 tests run: 99 passed, 0 skipped' >&2\n");
    script.push_str("printf '%s\\n' '");
    script.push_str(list_json);
    script.push_str("'\nexit 0\nfi\n");
    script.push_str(run);
    script.push('\n');
    fs::write(&program, script).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
}
pub(crate) fn listed(packages: &[(&str, &[&str])]) -> String {
    let mut count = 0_usize;
    let suites = packages
    .iter()
    .map(|(package, names)| {
        count += names.len();
        let cases = names
            .iter()
            .map(|name| {
                format!(
                    "\"{name}\":{{\"ignored\":false,\"filter-match\":{{\"status\":\"matches\"}}}}"
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "\"{package}\":{{\"package-name\":\"{package}\",\"status\":\"listed\",\"testcases\":{{{cases}}}}}"
        )
    })
    .collect::<Vec<_>>()
    .join(",");
    format!("{{\"test-count\":{count},\"rust-suites\":{{{suites}}}}}")
}
pub(crate) fn list_args(temp: &Path) -> Vec<String> {
    fs::read_to_string(temp.join("list-argv"))
        .unwrap_or_else(|error| unreachable!("{error}"))
        .lines()
        .map(ToOwned::to_owned)
        .collect()
}
pub(crate) fn assert_list_args(args: &[String], present: &[&str], absent: &[&str]) {
    for flag in present {
        assert!(
            args.iter().any(|arg| arg == flag),
            "missing {flag} from {args:?}"
        );
    }
    for flag in absent {
        assert!(
            args.iter().all(|arg| arg != flag),
            "{flag} leaked into {args:?}"
        );
    }
    assert_eq!(
        args.iter().filter(|arg| *arg == "--message-format").count(),
        1,
        "{args:?}"
    );
}
pub(crate) fn run_with_output(
    name: &str,
    run: &str,
    list_json: &str,
    command: &[&str],
) -> (Output, String, PathBuf) {
    let (output, written, temp, _) = execute_nextest(name, run, list_json, command, None);
    (output, written, temp)
}
/// The step output without the `built=` line, which a nextest run writes once its
/// build is known. Counts are what the held-pipe tests assert are withheld.
pub(crate) fn without_built(written: &str) -> String {
    let mut kept = String::new();
    for line in written.lines().filter(|line| !line.starts_with("built=")) {
        kept.push_str(line);
        kept.push('\n');
    }
    kept
}
pub(crate) fn execute_nextest(
    name: &str,
    run: &str,
    list_json: &str,
    command: &[&str],
    grace_ms: Option<&str>,
) -> (Output, String, PathBuf, std::time::Duration) {
    let started = std::time::Instant::now();
    let temp = scratch(name);
    fake_nextest(&temp, run, list_json);
    let outputs = temp.join("github-output");
    fs::write(&outputs, "existing=1\n").unwrap_or_else(|e| unreachable!("{e}"));
    let path = format!(
        "{}:{}",
        temp.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    child
        .arg("run")
        .arg("--")
        .args(command)
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", &outputs)
        .env("PATH", path);
    if let Some(grace_ms) = grace_ms {
        child.env("VELNOR_CI_RUN_PUMP_GRACE_MS", grace_ms);
    }
    let output = child.output().unwrap_or_else(|e| unreachable!("{e}"));
    let written = fs::read_to_string(&outputs).unwrap_or_default();
    (output, written, temp, started.elapsed())
}
pub(crate) const NEXTEST_RUN: &[&str] = &[
    "cargo-nextest",
    "nextest",
    "run",
    "--archive-file",
    "a.tar.zst",
    "--no-tests=pass",
];
pub(crate) const FILTERED_LIST: &str = concat!(
    r#"{"test-count":4,"rust-suites":{"#,
    r#""only-ign":{"package-name":"only-ign","status":"listed","testcases":{"hidden":{"ignored":true,"filter-match":{"status":"mismatch","reason":"ignored"}}}},"#,
    r#""kept":{"package-name":"kept","status":"listed","testcases":{"alpha":{"ignored":false,"filter-match":{"status":"matches"}},"beta":{"ignored":false,"filter-match":{"status":"mismatch","reason":"expression"}}}},"#,
    r#""both":{"package-name":"both","status":"listed","testcases":{"unit":{"ignored":false,"filter-match":{"status":"matches"}}}},"#,
    r#""both::plain":{"package-name":"both","status":"skipped","testcases":{}},"#,
    r#""gone":{"package-name":"gone","status":"skipped","testcases":{}}}}"#
);
pub(crate) const EXCLUDED_LIST: &str = concat!(
    r#"{"test-count":1,"rust-suites":{"gone":{"package-name":"gone","status":"listed","testcases":{"#,
    r#""only":{"ignored":false,"filter-match":{"status":"mismatch","reason":"expression"}}}}}}"#
);
pub(crate) const RUN_ONLY: &[&str] = &[
    "--no-fail-fast",
    "--status-level",
    "skip",
    "--no-tests",
    "--no-tests=pass",
    "run",
];

#[test]
fn run_appends_the_number_of_tests_nextest_ran_to_the_step_output() {
    let split = format!(
        "{{ printf '     Summary [   0.020s] {BOLD}7'; sleep 0.3; printf '{RESET} tests run: 7 passed, 0 skipped\\n'; }} >&2"
    );
    let seven = listed(&[("kept", &["a", "b", "c", "d", "e", "f", "g"])]);
    let (output, written, _) = run_with_output("tests-run-split", &split, &seven, NEXTEST_RUN);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        written,
        "existing=1\nbuilt=true\ntests_run=7\ntests_listed=7\ntests_ignored=0\n"
    );
}

#[test]
fn run_counts_failed_tests_and_keeps_the_exit_code_of_nextest() {
    let script = format!(
        "echo '     Summary [   1.200s] {BOLD}5{RESET} tests run: 3 passed, {BOLD}2{RESET} failed, 0 skipped' >&2; exit 100"
    );
    let five = listed(&[("kept", &["a", "b", "c", "d", "e"])]);
    let (output, written, _) = run_with_output("tests-run-failed", &script, &five, NEXTEST_RUN);
    assert_eq!(output.status.code(), Some(100));
    assert_eq!(
        written,
        "existing=1\nbuilt=true\ntests_run=5\ntests_listed=5\ntests_ignored=0\n"
    );
}

#[test]
fn run_reports_a_run_without_tests_as_zero() {
    let script = "echo '     Summary [   0.001s] 0 tests run: 0 passed, 0 skipped' >&2";
    let none = listed(&[]);
    let (output, written, _) = run_with_output("tests-run-zero", script, &none, NEXTEST_RUN);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        written,
        "existing=1\nbuilt=true\ntests_run=0\ntests_listed=0\ntests_ignored=0\ntests_by_package=\n"
    );
}

#[test]
fn run_omits_a_package_whose_tests_the_filter_excluded() {
    let script = r"cat >&2 <<'END'
SKIP [         ] (───) gone only
 Summary [   0.001s] 0 tests run: 0 passed, 1 skipped
END";
    let (output, written, _) =
        run_with_output("tests-run-filtered", script, EXCLUDED_LIST, NEXTEST_RUN);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        written,
        "existing=1\nbuilt=true\ntests_run=0\ntests_listed=1\ntests_ignored=0\ntests_by_package=gone=0/1\n"
    );
}

#[test]
fn run_counts_status_lines_without_a_progress_counter() {
    let script = r"cat >&2 <<'END'
PASS [   0.012s] kept alpha
PASS [   0.012s] kept beta
 Summary [   0.022s] 2 tests run: 2 passed, 0 skipped
END";
    let two = listed(&[("kept", &["alpha", "beta"])]);
    let (_, written, _) = run_with_output("tests-run-no-counter", script, &two, NEXTEST_RUN);
    assert_eq!(
        written,
        "existing=1\nbuilt=true\ntests_run=2\ntests_listed=2\ntests_ignored=0\ntests_by_package=kept=2/2\n"
    );
}

#[test]
fn run_counts_a_test_that_passed_on_a_retry_once() {
    let script = r"cat >&2 <<'END'
PASS [   0.012s] (1/2) kept alpha
 TRY 1 FAIL [   0.012s] (2/2) kept beta
  TRY 2 PASS [   0.012s] (2/2) kept beta
 Summary [   0.022s] 2 tests run: 2 passed (1 flaky), 0 skipped
   FLAKY 2/3 [   0.016s] (2/2) kept beta
END";
    let two = listed(&[("kept", &["alpha", "beta"])]);
    let (_, written, _) = run_with_output("tests-run-retry", script, &two, NEXTEST_RUN);
    assert_eq!(
        written,
        "existing=1\nbuilt=true\ntests_run=2\ntests_listed=2\ntests_ignored=0\ntests_by_package=kept=2/2\n"
    );
}

#[test]
fn run_counts_a_package_with_a_unicode_name() {
    let script = r"cat >&2 <<'END'
PASS [   0.012s] (1/1) café t::ok
 Summary [   0.012s] 1 test run: 1 passed, 0 skipped
END";
    let one = listed(&[("café", &["t::ok"])]);
    let (_, written, _) = run_with_output("tests-run-unicode", script, &one, NEXTEST_RUN);
    assert_eq!(
        written,
        "existing=1\nbuilt=true\ntests_run=1\ntests_listed=1\ntests_ignored=0\ntests_by_package=café=1/1\n"
    );
}

#[test]
fn run_counts_a_status_line_longer_than_the_line_limit() {
    let name = "t".repeat(5000);
    let script = format!(
        "{{ printf '        PASS [   0.012s] (1/1) kept {name}\\n'; echo '     Summary [   0.012s] 1 test run: 1 passed, 0 skipped'; }} >&2"
    );
    let one = listed(&[("kept", &["x"])]);
    let (_, written, _) = run_with_output("tests-run-long", &script, &one, NEXTEST_RUN);
    assert_eq!(
        written,
        "existing=1\nbuilt=true\ntests_run=1\ntests_listed=1\ntests_ignored=0\ntests_by_package=kept=1/1\n"
    );
}

#[test]
fn run_counts_runnable_tests_when_the_log_has_no_status_lines() {
    let mixed = "echo '     Summary [   0.5s] 3 tests run: 2 passed, 1 failed, 5 skipped' >&2";
    let three = listed(&[("kept", &["a", "b", "c"])]);
    let (_, written, _) = run_with_output("tests-run-mixed", mixed, &three, NEXTEST_RUN);
    assert_eq!(
        written,
        "existing=1\nbuilt=true\ntests_run=3\ntests_listed=3\ntests_ignored=0\n"
    );
    let bare = "echo '     Summary [   0.5s] 3 tests run: 3 passed' >&2";
    let (_, written, _) = run_with_output("tests-run-bare", bare, &three, NEXTEST_RUN);
    assert_eq!(
        written,
        "existing=1\nbuilt=true\ntests_run=3\ntests_listed=3\ntests_ignored=0\n"
    );
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
        let (_, written, _) =
            run_with_output(&format!("tests-run-no-{name}"), script, "", &command);
        assert_eq!(without_built(&written), "existing=1\n", "{name}");
    }
}

#[test]
fn run_ignores_captured_output_that_looks_like_a_status_line() {
    let script = "cat >&2 <<'END'\n\
    PASS [   0.012s] (1/1) kept alpha\n\
  stdout \u{2500}\u{2500}\u{2500}\n\
\n\
running 1 test\n\
PASS [   0.012s] kept tests::fake\n\
test alpha ... ok\n\
\n\
 Summary [   0.012s] 1 test run: 1 passed, 0 skipped\n\
END";
    let one = listed(&[("kept", &["alpha"])]);
    let (output, written, _) = run_with_output("tests-run-captured", script, &one, NEXTEST_RUN);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        written,
        "existing=1\nbuilt=true\ntests_run=1\ntests_listed=1\ntests_ignored=0\ntests_by_package=kept=1/1\n"
    );
}

#[test]
fn run_counts_a_run_whose_setup_script_printed_status_lines() {
    let script = r"cat >&2 <<'END'
    Starting 2 tests across 1 binary
     SETUP [      1/1] prepull-images: bash .config/nextest-prepull-images.sh
nextest-prepull-images: pulling postgres:18-alpine
 SETUP PASS [   9.804s] prepull-images: bash .config/nextest-prepull-images.sh
        PASS [   0.012s] (1/2) kept alpha
        PASS [   0.012s] (2/2) kept beta
     Summary [   0.022s] 2 tests run: 2 passed, 0 skipped
END";
    let two = listed(&[("kept", &["alpha", "beta"])]);
    let (_, written, _) = run_with_output("tests-run-setup-script", script, &two, NEXTEST_RUN);
    assert_eq!(
        written,
        "existing=1\nbuilt=true\ntests_run=2\ntests_listed=2\ntests_ignored=0\ntests_by_package=kept=2/2\n"
    );
}
