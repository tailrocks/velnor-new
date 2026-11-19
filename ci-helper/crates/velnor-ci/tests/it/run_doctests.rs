// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::{Command, Output};

use crate::run::{run, scratch};

fn doctest_run(temp: &Path, script: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--require-doctests", "--", "sh", "-c", script])
        .env("RUNNER_TEMP", temp)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_doctest_step_that_ran_zero_doctests_fails() {
    let temp = scratch("doctests-zero");
    let zero = "printf '   Doc-tests a\\n\\nrunning 0 tests\\n\\ntest result: ok. 0 passed\\n'";
    let output = doctest_run(&temp, zero);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("0 doctests"),
        "{output:?}"
    );
    assert_eq!(
        output.stdout,
        "   Doc-tests a\n\nrunning 0 tests\n\ntest result: ok. 0 passed\n".as_bytes()
    );
    let none = doctest_run(&temp, "true");
    assert_eq!(none.status.code(), Some(1), "{none:?}");
}

#[test]
fn a_doctest_step_that_ran_a_doctest_passes_and_a_failing_one_keeps_its_code() {
    let temp = scratch("doctests-some");
    let ran = "printf 'running 0 tests\\nrunning 1 test\\ntest x ... ok\\n'";
    assert_eq!(doctest_run(&temp, ran).status.code(), Some(0));
    let split = "printf 'running 0 tests\\nrunn'; printf 'ing 3 tests\\n'";
    assert_eq!(doctest_run(&temp, split).status.code(), Some(0));
    let failing = "printf 'running 0 tests\\n'; exit 101";
    assert_eq!(doctest_run(&temp, failing).status.code(), Some(101));
}

#[test]
fn a_doctest_flag_set_to_false_does_not_require_a_doctest() {
    let temp = scratch("doctests-false");
    for (value, code) in [("false", 0), ("true", 1)] {
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["run", &format!("--require-doctests={value}"), "--", "true"])
            .env("RUNNER_TEMP", &temp)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(output.status.code(), Some(code), "{value} {output:?}");
    }
}

#[test]
fn a_run_without_the_flag_does_not_count_doctests() {
    let temp = scratch("doctests-off");
    let output = run(&temp, &["sh", "-c", "printf 'running 0 tests\\n'"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}
