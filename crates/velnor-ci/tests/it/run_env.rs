// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const STEP_FILES: &[&str] = &[
    "GITHUB_OUTPUT",
    "GITHUB_ENV",
    "GITHUB_PATH",
    "GITHUB_STEP_SUMMARY",
    "GITHUB_STATE",
];

fn scratch(name: &str) -> PathBuf {
    let dir = crate::scratch_path("velnor-ci", name);
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    dir
}

fn run_with_step_files(temp: &Path, command: &[&str]) -> bool {
    let outputs = temp.join("github-output");
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    let path = format!(
        "{}:{}",
        temp.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut run = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    run.arg("run")
        .arg("--")
        .args(command)
        .env("RUNNER_TEMP", temp)
        .env("PATH", path);
    for name in STEP_FILES.iter().copied() {
        run.env(name, &outputs);
    }
    run.output()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .status
        .success()
}

fn leaked(env_dump: &str) -> Vec<&'static str> {
    STEP_FILES
        .iter()
        .copied()
        .filter(|name| {
            env_dump
                .lines()
                .any(|line| line.starts_with(&format!("{name}=")))
        })
        .collect()
}

#[test]
fn run_starts_nextest_without_the_step_file_variables() {
    let temp = scratch("run-env-nextest");
    let program = temp.join("cargo-nextest");
    fs::write(
        &program,
        "#!/bin/sh\nenv > \"$RUNNER_TEMP/child-env\"\necho '     Summary [   0.001s] 1 test run: 1 passed, 0 skipped' >&2\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(run_with_step_files(
        &temp,
        &["cargo-nextest", "nextest", "run", "--no-tests=pass"]
    ));
    let seen = fs::read_to_string(temp.join("child-env")).unwrap_or_default();
    assert!(seen.contains("RUNNER_TEMP="), "{seen}");
    assert_eq!(leaked(&seen), Vec::<&str>::new(), "{seen}");
}

#[test]
fn run_leaves_the_step_file_variables_to_commands_that_are_not_nextest() {
    let temp = scratch("run-env-other");
    assert!(run_with_step_files(
        &temp,
        &["sh", "-c", "env > \"$RUNNER_TEMP/child-env\""]
    ));
    let seen = fs::read_to_string(temp.join("child-env")).unwrap_or_default();
    assert_eq!(leaked(&seen), STEP_FILES.to_vec(), "{seen}");
}

fn fake_cargo(temp: &Path) {
    let program = temp.join("cargo");
    fs::write(
        &program,
        "#!/bin/sh\nprintf '%s' \"${CARGO_LOG-unset}\" > \"$RUNNER_TEMP/cargo-log\"\necho '   0.024s  INFO prepare_target{force=false package_id=gen v0.1.0 (/work/gen) target=\"gen\"}: cargo::compiler::fingerprint:     dirty: FsStatusOutdated(StaleItem(MissingFile { path: \"/store/x.rs\" }))' >&2\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn cargo_log_seen(temp: &Path, command: &[&str], preset: Option<&str>) -> String {
    fs::remove_file(temp.join("velnor-invocations.jsonl")).ok();
    let path = format!(
        "{}:{}",
        temp.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut run = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    run.arg("run")
        .arg("--")
        .args(command)
        .env("RUNNER_TEMP", temp)
        .env("PATH", path)
        .env_remove("CARGO_LOG");
    if let Some(preset) = preset {
        run.env("CARGO_LOG", preset);
    }
    assert!(
        run.output()
            .unwrap_or_else(|e| unreachable!("{e}"))
            .status
            .success()
    );
    fs::read_to_string(temp.join("cargo-log")).unwrap_or_default()
}

#[test]
fn a_cargo_build_command_runs_with_the_fingerprint_log_and_records_cargos_dirty_reason() {
    let temp = scratch("run-env-fingerprint-log");
    fake_cargo(&temp);
    for subcommand in ["check", "clippy", "build", "doc"] {
        assert_eq!(
            cargo_log_seen(&temp, &["cargo", subcommand, "--locked"], None),
            "cargo::compiler::fingerprint=info",
            "{subcommand}"
        );
    }
    let records = fs::read_to_string(temp.join("velnor-invocations.jsonl")).unwrap_or_default();
    assert!(
        records.contains(
            r#""dirty":[{"package":"gen","version":"0.1.0","reason":"FsStatusOutdated(StaleItem(MissingFile { path: \"/store/x.rs\" }))"}]"#
        ),
        "{records}"
    );
}

#[test]
fn the_fingerprint_log_is_kept_off_commands_that_run_tests_and_never_overrides_the_caller() {
    let temp = scratch("run-env-fingerprint-log-off");
    fake_cargo(&temp);
    for command in [
        &["cargo", "test"][..],
        &["cargo", "run"][..],
        &["cargo", "nextest", "run"][..],
    ] {
        assert_eq!(cargo_log_seen(&temp, command, None), "unset", "{command:?}");
    }
    assert_eq!(
        cargo_log_seen(&temp, &["cargo", "check"], Some("warn")),
        "warn"
    );
}
