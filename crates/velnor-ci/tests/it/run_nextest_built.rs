// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use crate::run_nextest::{NEXTEST_RUN, execute_nextest};

fn built_line(name: &str, code: i32) -> String {
    let script = format!("exit {code}");
    let (output, written, _, _) = execute_nextest(name, &script, "", &NEXTEST_RUN, None);
    assert_eq!(output.status.code(), Some(code), "{output:?}");
    written
        .lines()
        .filter(|line| line.starts_with("built="))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_passing_run_records_that_the_build_finished() {
    assert_eq!(built_line("built-pass", 0), "built=true");
}

#[test]
fn a_run_whose_tests_failed_records_that_the_build_finished() {
    assert_eq!(built_line("built-tests-failed", 100), "built=true");
}

#[test]
fn a_build_failure_records_that_the_build_did_not_finish() {
    assert_eq!(built_line("built-build-failed", 101), "built=false");
}

#[test]
fn an_unexpected_failure_records_that_the_build_did_not_finish() {
    assert_eq!(built_line("built-unexpected", 1), "built=false");
}
