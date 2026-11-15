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
        text.matches("velnor-ci run: sh -c").count(),
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
