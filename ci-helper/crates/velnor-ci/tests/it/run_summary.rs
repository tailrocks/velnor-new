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
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let written = fs::read_to_string(temp.join("github-output")).unwrap_or_default();
    (output, written)
}

fn doctests(name: &str, body: &str) -> Output {
    let temp = scratch(name);
    velnor(&temp, &["--require-doctests"], &["sh", "-c", body])
        .env("VELNOR_CI_RUN_PUMP_GRACE_MS", "300")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
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
fn a_doctest_line_still_being_written_when_the_grace_ends_is_not_counted() {
    let body = "printf 'running 2'; (sleep 3; printf -- '-not\\n') & exit 0";
    let output = doctests("partial", body);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

#[test]
fn a_doctest_line_without_a_newline_at_end_of_stream_is_counted() {
    let output = doctests("eof", "printf 'running 2 tests'");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
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
        if started.elapsed() > Duration::from_secs(5) {
            child.kill().ok();
            child.wait().ok();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.and_then(|status| status.code()), Some(2));
    assert!(!marker.exists());
}
