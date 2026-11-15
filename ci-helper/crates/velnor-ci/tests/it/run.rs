// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("velnor-ci-run-{}-{name}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    }
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    dir
}

fn run(temp: &Path, command: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("run")
        .arg("--")
        .args(command)
        .env("RUNNER_TEMP", temp)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn log(temp: &Path) -> String {
    fs::read_to_string(temp.join("velnor-run.log")).unwrap_or_default()
}

#[test]
fn run_exits_with_the_exit_code_of_the_command() {
    let temp = scratch("exit");
    assert_eq!(run(&temp, &["false"]).status.code(), Some(1));
    assert_eq!(run(&temp, &["true"]).status.code(), Some(0));
    assert_eq!(run(&temp, &["sh", "-c", "exit 7"]).status.code(), Some(7));
    assert_eq!(
        run(&temp, &["sh", "-c", "kill -TERM $$"]).status.code(),
        Some(143)
    );
}

#[test]
fn run_prints_cargos_version_unchanged_and_appends_it_to_the_log() {
    let temp = scratch("cargo");
    let output = run(&temp, &["cargo", "--version"]);
    let version = Command::new("cargo")
        .arg("--version")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    assert!(version.stdout.starts_with(b"cargo "));
    assert_eq!(output.stdout, version.stdout);
    assert!(log(&temp).contains(String::from_utf8_lossy(&version.stdout).trim()));
}

#[test]
fn run_keeps_each_stream_and_every_byte_and_appends_one_record_per_command() {
    let temp = scratch("streams");
    let big = "x".repeat(100_000);
    let script = format!("printf 'no newline'; printf 'warn' >&2; printf '{big}'");
    let first = run(&temp, &["sh", "-c", &script]);
    assert_eq!(
        String::from_utf8_lossy(&first.stdout),
        format!("no newline{big}")
    );
    assert_eq!(first.stderr, b"warn");
    run(&temp, &["sh", "-c", "exit 3"]);
    let text = log(&temp);
    assert_eq!(
        text.matches("velnor-ci run: sh (2 arguments)").count(),
        2,
        "{}",
        text.len()
    );
    assert!(text.contains("no newline"));
    assert!(text.ends_with("velnor-ci run: exit 3\n"), "{text}");
    assert!(text.contains("velnor-ci run: exit 0\n"));
}

#[test]
fn run_names_a_program_it_cannot_start_and_exits_127() {
    let temp = scratch("missing");
    let output = run(&temp, &["velnor-no-such-program"]);
    assert_eq!(output.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&output.stderr).contains("velnor-no-such-program"));
}

#[test]
fn run_does_not_start_the_command_when_the_log_cannot_be_written() {
    let temp = scratch("unwritable").join("missing-folder");
    let marker = scratch("unwritable-marker").join("started");
    let output = run(&temp, &["touch", marker.to_string_lossy().as_ref()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!marker.exists());
}

#[test]
fn run_without_a_command_is_a_usage_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("run")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn run_passes_every_argument_after_the_first_separator_to_the_command() {
    let temp = scratch("separator");
    let output = run(&temp, &["echo", "clippy", "--", "-D", "warnings", "--help"]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "clippy -- -D warnings --help\n"
    );
}

#[test]
fn run_passes_shell_metacharacters_to_the_command_as_plain_arguments() {
    let temp = scratch("metacharacters");
    let marker = temp.join("touched");
    let marker = marker.to_string_lossy();
    let semicolon = format!("; touch {marker}");
    let substitution = format!("$(touch {marker})");
    let backticks = format!("`touch {marker}`");
    let quotes = "\"double\" 'single' \\ * ? | & > <";
    let arguments = [
        "printf",
        "[%s]\n",
        &semicolon,
        &substitution,
        &backticks,
        quotes,
    ];
    let output = run(&temp, &arguments);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("[{semicolon}]\n[{substitution}]\n[{backticks}]\n[{quotes}]\n")
    );
    assert!(!temp.join("touched").exists());
}

#[test]
fn run_logs_the_program_and_the_argument_count_but_never_the_arguments() {
    let temp = scratch("redaction");
    let output = run(&temp, &["true", "token-abc123", "--password=hunter2"]);
    assert!(output.status.success(), "{output:?}");
    let text = log(&temp);
    assert!(
        text.contains("velnor-ci run: true (2 arguments)\n"),
        "{text}"
    );
    assert!(
        !text.contains("token-abc123") && !text.contains("hunter2"),
        "{text}"
    );
}

#[test]
fn run_fails_without_starting_the_command_when_runner_temp_is_unset() {
    let marker = scratch("no-temp").join("started");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--", "touch"])
        .arg(&marker)
        .env_remove("RUNNER_TEMP")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("RUNNER_TEMP"));
    assert!(!marker.exists());
}

#[cfg(unix)]
#[test]
fn run_refuses_a_log_that_is_a_symlink_and_leaves_its_target_alone() {
    let temp = scratch("symlink");
    let target = scratch("symlink-target").join("victim");
    fs::write(&target, "kept").unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(&target, temp.join("velnor-run.log"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let marker = temp.join("started");
    let output = run(&temp, &["touch", marker.to_string_lossy().as_ref()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!marker.exists());
    assert_eq!(fs::read_to_string(&target).unwrap_or_default(), "kept");
}

#[cfg(unix)]
mod tests_run {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    use super::scratch;

    const BOLD: &str = "\u{1b}[1m";
    const RESET: &str = "\u{1b}[0m";

    fn fake_nextest(directory: &Path, script: &str) {
        let program = directory.join("cargo-nextest");
        fs::write(&program, format!("#!/bin/sh\n{script}\n"))
            .unwrap_or_else(|e| unreachable!("{e}"));
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
            let (_, written, _) =
                run_with_output(&format!("tests-run-no-{name}"), script, &command);
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
}
