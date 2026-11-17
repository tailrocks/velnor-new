// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const RATIO: &str = "echo '     Summary [   0.077s] 63/109 tests run: 62 passed, 1 failed, 0 skipped' >&2\nexit 100";
const NEXTEST: [&str; 3] = ["cargo-nextest", "nextest", "run"];

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("velnor-ci-sum-{}-{name}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    dir
}

fn script(temp: &Path, name: &str, body: &str) {
    let program = temp.join(name);
    fs::write(&program, format!("#!/bin/sh\n{body}\n")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn velnor(temp: &Path, flags: &[&str], command: &[&str]) -> Command {
    let path = format!(
        "{}:{}",
        temp.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut run = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    run.arg("run")
        .args(flags)
        .arg("--")
        .args(command)
        .env("RUNNER_TEMP", temp)
        .env("GITHUB_OUTPUT", temp.join("github-output"))
        .env("PATH", path);
    run
}

fn nextest(name: &str, body: &str) -> (Output, String) {
    let temp = scratch(name);
    script(&temp, "cargo-nextest", body);
    fs::write(temp.join("github-output"), "").unwrap_or_else(|e| unreachable!("{e}"));
    let output = velnor(&temp, &[], &NEXTEST)
        .env("VELNOR_CI_RUN_PUMP_GRACE_MS", "300")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let written = fs::read_to_string(temp.join("github-output")).unwrap_or_default();
    (output, written)
}

fn doctests_in(temp: &Path, body: &str) -> Output {
    velnor(temp, &["--require-doctests"], &["sh", "-c", body])
        .env("VELNOR_CI_RUN_PUMP_GRACE_MS", "300")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn doctests(name: &str, body: &str) -> Output {
    doctests_in(&scratch(name), body)
}

#[test]
fn a_nextest_ratio_summary_publishes_the_finished_count_and_keeps_the_exit_code() {
    let (output, written) = nextest("ratio", RATIO);
    assert_eq!(output.status.code(), Some(100), "{output:?}");
    assert!(written.contains("tests_run=63\n"), "{written}");
}

#[test]
fn a_cancelled_nextest_run_publishes_the_finished_count() {
    let body = "echo 'Summary [   0.200s] 3/8 tests run: 3 passed, 0 skipped' >&2";
    let (_, written) = nextest("cancelled", body);
    assert!(written.contains("tests_run=3\n"), "{written}");
}

#[test]
fn a_later_stdout_summary_does_not_replace_the_stderr_count() {
    let body = "echo '     Summary [   0.020s] 9 tests run: 9 passed, 0 skipped' >&2\n\
exec 2>&-\n\
sleep 0.3\n\
echo '     Summary [   0.001s] 0 tests run: 0 passed, 0 skipped'";
    let (output, written) = nextest("stream-order", body);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(written.contains("tests_run=9\n"), "{written}");
    assert!(!written.contains("tests_run=0"), "{written}");
}

#[test]
fn a_count_read_before_a_descendant_holds_the_pipe_is_published_after_the_grace() {
    let body = "echo '     Summary [   0.020s] 4 tests run: 4 passed, 0 skipped' >&2\n\
sleep 30 &\n\
exit 0";
    let (output, written) = nextest("held", body);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(written.contains("tests_run=4\n"), "{written}");
}

#[test]
fn a_doctest_step_that_ran_zero_doctests_fails() {
    let temp = scratch("doctests-zero");
    let zero = "printf '   Doc-tests a\\n\\nrunning 0 tests\\n\\ntest result: ok. 0 passed\\n'";
    let output = doctests_in(&temp, zero);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("0 doctests"),
        "{output:?}"
    );
    assert_eq!(
        output.stdout,
        "   Doc-tests a\n\nrunning 0 tests\n\ntest result: ok. 0 passed\n".as_bytes()
    );
    let none = doctests_in(&temp, "true");
    assert_eq!(none.status.code(), Some(1), "{none:?}");
}

#[test]
fn a_doctest_step_that_ran_a_doctest_passes_and_a_failing_one_keeps_its_code() {
    let temp = scratch("doctests-some");
    let ran = "printf 'running 0 tests\\ntest result: ok. 0 passed\\nrunning 1 test\\ntest x ... ok\\ntest result: ok. 1 passed\\n'";
    assert_eq!(doctests_in(&temp, ran).status.code(), Some(0));
    let split = "printf 'running 0 tests\\ntest result: ok. 0 passed\\nrunn'; printf 'ing 3 tests\\ntest result: ok. 3 passed\\n'";
    assert_eq!(doctests_in(&temp, split).status.code(), Some(0));
    let failing = "printf 'running 0 tests\\n'; exit 101";
    assert_eq!(doctests_in(&temp, failing).status.code(), Some(101));
}

#[test]
fn a_doctest_flag_set_to_false_does_not_require_a_doctest() {
    let temp = scratch("doctests-false");
    for (value, code) in [("false", 0), ("true", 1)] {
        let output = velnor(&temp, &[&format!("--require-doctests={value}")], &["true"])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(output.status.code(), Some(code), "{value} {output:?}");
    }
}

#[test]
fn a_run_without_the_flag_does_not_count_doctests() {
    let temp = scratch("doctests-off");
    let output = velnor(&temp, &[], &["sh", "-c", "printf 'running 0 tests\\n'"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn an_ignored_doctest_does_not_count_as_a_doctest_that_ran() {
    let ignored = "printf 'running 1 test\\ntest src/lib.rs - example (line 1) ... ignored\\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\\n'";
    let output = doctests("ignored", ignored);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let passed = "printf 'running 1 test\\ntest src/lib.rs - example (line 1) ... ok\\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\n'";
    assert_eq!(doctests("passed", passed).status.code(), Some(0));
    let mixed = "printf 'running 2 tests\\ntest result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\\n'";
    assert_eq!(doctests("mixed", mixed).status.code(), Some(0));
}

#[test]
fn colored_doctest_result_lines_commit_running_counts_before_the_pipe_grace() {
    let passed = "printf '\\033[32mrunning 1 test\\033[0m\\n\\033[32mtest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\033[0m\\n'; sleep 3 & exit 0";
    assert_eq!(doctests("colored-passed", passed).status.code(), Some(0));
    let ignored = "printf '\\033[32mrunning 1 test\\033[0m\\n\\033[32mtest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\\033[0m\\n'; sleep 3 & exit 0";
    assert_eq!(doctests("colored-ignored", ignored).status.code(), Some(1));
}

#[test]
fn a_doctest_line_still_being_written_when_the_grace_ends_is_not_counted() {
    let body = "printf 'running 2'; (sleep 3; printf -- '-not\\n') & exit 0";
    let output = doctests("partial", body);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

#[test]
fn a_doctest_header_without_a_result_does_not_count_at_end_of_stream() {
    let output = doctests("eof", "printf 'running 2 tests'");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

#[test]
fn a_truncated_doctest_transcript_does_not_count_as_a_doctest_that_ran() {
    let truncated = "printf 'running 1 test\\ntest src/lib.rs - example (line 1) ... ok\\n'";
    let output = doctests("truncated", truncated);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

#[test]
fn an_ignored_doctest_without_a_result_line_does_not_count_at_end_of_stream() {
    let ignored = "printf 'running 1 test\\ntest src/lib.rs - example (line 1) ... ignored\\n'";
    let output = doctests("ignored-eof", ignored);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

#[test]
fn a_doctest_result_without_a_trailing_newline_counts_at_end_of_stream() {
    let body = "printf 'running 1 test\\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out'";
    let output = doctests("eof-result", body);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn a_closed_console_with_a_descendant_holding_the_pipe_preserves_the_write_error() {
    let temp = scratch("console-held");
    let (reader, writer) = std::io::pipe().unwrap_or_else(|e| unreachable!("{e}"));
    drop(reader);
    let output = velnor(&temp, &[], &["sh", "-c", "echo hello; sleep 3 & exit 0"])
        .stdout(writer)
        .stderr(Stdio::piped())
        .env("VELNOR_CI_RUN_PUMP_GRACE_MS", "300")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot write the command output"),
        "{output:?}"
    );
}

#[test]
fn a_running_doctest_line_pending_at_grace_does_not_count_without_a_result() {
    let body = "printf 'running 1 test\\n'; (sleep 3; printf 'test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\\n') & exit 0";
    let output = doctests("pending-result", body);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

#[test]
fn a_failed_console_write_fails_a_command_that_exited_zero() {
    let temp = scratch("console");
    let (reader, writer) = std::io::pipe().unwrap_or_else(|e| unreachable!("{e}"));
    drop(reader);
    let output = velnor(&temp, &[], &["printf", "hello\\n"])
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let log = fs::read_to_string(temp.join("velnor-run.log")).unwrap_or_default();
    assert!(log.contains("hello"), "{log}");
}

#[test]
fn a_failed_console_write_keeps_the_exit_code_of_a_failing_command() {
    let temp = scratch("console-failing");
    let (reader, writer) = std::io::pipe().unwrap_or_else(|e| unreachable!("{e}"));
    drop(reader);
    let output = velnor(&temp, &[], &["sh", "-c", "echo hello; exit 7"])
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(7), "{output:?}");
}

#[test]
fn a_log_that_is_a_fifo_without_a_reader_is_refused_without_starting_the_command() {
    let temp = scratch("fifo");
    let marker = temp.join("started");
    let made = Command::new("mkfifo")
        .arg(temp.join("velnor-run.log"))
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(made.success());
    let mut child = velnor(&temp, &[], &["touch", marker.to_string_lossy().as_ref()])
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap_or_else(|e| unreachable!("{e}")) {
            break Some(status);
        }
        if started.elapsed() > Duration::from_secs(1) {
            child.kill().ok();
            child.wait().ok();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.and_then(|status| status.code()), Some(2));
    assert!(!marker.exists());
}
