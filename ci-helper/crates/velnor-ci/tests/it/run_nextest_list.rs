// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use crate::run_nextest::{RUN_ONLY, assert_list_args, list_args, listed, run_with_output};

#[test]
fn run_lists_without_any_option_only_nextest_run_accepts() {
    assert_scope(
        "tests-run-list-reporter",
        &[
            "cargo-nextest",
            "nextest",
            "run",
            "--workspace",
            "--failure-output",
            "never",
            "--success-output=final",
            "--no-output-indent",
            "--max-fail",
            "3",
            "--test-threads",
            "2",
            "--flaky-result=pass",
            "--hide-progress-bar",
            "-E",
            "all()",
        ],
        &["list", "--workspace", "-E", "all()"],
        &[
            "--failure-output",
            "never",
            "--success-output=final",
            "--no-output-indent",
            "--max-fail",
            "--test-threads",
            "--flaky-result=pass",
            "--hide-progress-bar",
        ],
    );
}

fn assert_scope(name: &str, command: &[&str], present: &[&str], absent: &[&str]) {
    let script = "echo '        PASS [   0.012s] (1/1) kept alpha' >&2\necho '     Summary [   0.012s] 1 test run: 1 passed, 0 skipped' >&2";
    let one = listed(&[("kept", &["alpha"])]);
    let (output, written, temp) = run_with_output(name, script, &one, command);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        written,
        "existing=1\ntests_run=1\ntests_listed=1\ntests_ignored=0\ntests_by_package=kept=1/1\n"
    );
    let args = list_args(&temp);
    assert_list_args(&args, present, absent);
    assert_list_args(&args, &[], RUN_ONLY);
}

#[test]
fn run_lists_the_workspace_scope_without_run_only_flags() {
    assert_scope(
        "tests-run-list-workspace",
        &[
            "cargo-nextest",
            "nextest",
            "run",
            "--workspace",
            "--locked",
            "--no-fail-fast",
            "--ignore-default-filter",
            "--status-level",
            "skip",
            "-E",
            "all()",
            "--no-tests=pass",
        ],
        &[
            "list",
            "--workspace",
            "--locked",
            "--ignore-default-filter",
            "-E",
            "all()",
            "--message-format",
            "json",
        ],
        &[],
    );
}

#[test]
fn run_lists_the_archive_scope_without_run_only_flags() {
    assert_scope(
        "tests-run-list-archive",
        &[
            "cargo-nextest",
            "nextest",
            "run",
            "--archive-file",
            "a.tar.zst",
            "--extract-to",
            ".",
            "--extract-overwrite",
            "--workspace-remap",
            ".",
            "--no-fail-fast",
            "--ignore-default-filter",
            "--status-level",
            "skip",
            "-E",
            "(all()) - binary_id(=custom::plain)",
            "--no-tests=pass",
        ],
        &[
            "list",
            "--archive-file",
            "a.tar.zst",
            "--extract-to",
            ".",
            "--extract-overwrite",
            "--workspace-remap",
            "--ignore-default-filter",
            "-E",
            "(all()) - binary_id(=custom::plain)",
            "--message-format",
            "json",
        ],
        &["--locked", "--workspace"],
    );
}
