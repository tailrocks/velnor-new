// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const STEP_FILES: [&str; 5] = [
    "GITHUB_OUTPUT",
    "GITHUB_ENV",
    "GITHUB_PATH",
    "GITHUB_STEP_SUMMARY",
    "GITHUB_STATE",
];

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("velnor-ci-{name}-{}", std::process::id()));
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
    for name in STEP_FILES {
        run.env(name, &outputs);
    }
    run.output()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .status
        .success()
}

fn leaked(env_dump: &str) -> Vec<&'static str> {
    STEP_FILES
        .into_iter()
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
