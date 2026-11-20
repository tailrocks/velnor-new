// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::process::Command;

use crate::run::scratch;
use crate::run_nextest::{NEXTEST_RUN, execute_nextest, fake_nextest, listed, without_built};

#[test]
fn run_withholds_counts_and_packages_when_a_descendant_keeps_the_summary_pipe_open() {
    let script = "echo '     Summary [   0.001s] 3 tests run: 3 passed, 0 skipped' >&2\n\
sleep 30 &\n\
exit 0";
    let three = listed(&[("kept", &["a", "b", "c"])]);
    let (output, written, _, elapsed) =
        execute_nextest("tests-run-held", script, &three, &NEXTEST_RUN, Some("300"));
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(without_built(&written), "existing=1\n");
    assert!(elapsed < std::time::Duration::from_secs(15), "{elapsed:?}");
}

#[test]
fn run_writes_no_counts_when_a_held_pipe_never_printed_a_summary() {
    let script = "sleep 30 &\nexit 9";
    let (output, written, _, elapsed) = execute_nextest(
        "tests-run-held-empty",
        script,
        "",
        &NEXTEST_RUN,
        Some("300"),
    );
    assert_eq!(output.status.code(), Some(9), "{output:?}");
    assert_eq!(without_built(&written), "existing=1\n");
    assert!(elapsed < std::time::Duration::from_secs(15), "{elapsed:?}");
}

#[test]
fn run_does_not_need_a_step_output_file() {
    let temp = scratch("tests-run-no-output");
    let two = listed(&[("ran", &["a", "b"])]);
    fake_nextest(
        &temp,
        "echo '     Summary [ 0.1s] 2 tests run: 2 passed' >&2",
        &two,
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
