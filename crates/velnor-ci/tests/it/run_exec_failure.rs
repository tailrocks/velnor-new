// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use crate::run::{run, scratch};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const DENIED_LINE: &str =
    "[double-spawn] failed to exec \"{target}\": Permission denied (os error 13)";

fn fake_nextest(directory: &Path, stderr: &str, code: u8) -> String {
    let bin = directory.join("bin");
    fs::create_dir_all(&bin).unwrap_or_else(|e| unreachable!("{e}"));
    let program = bin.join("cargo");
    let script = format!("#!/bin/sh\nprintf '%s\\n' '{stderr}' >&2\nexit {code}\n");
    fs::write(&program, script).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    program.display().to_string()
}

fn log(temp: &Path) -> String {
    fs::read_to_string(temp.join("velnor-run.log")).unwrap_or_default()
}

#[test]
fn exec_denied_test_step_logs_stat_and_mount_and_keeps_exit_code() {
    let temp = scratch("exec-failure");
    let target = temp.join("target/debug/build/demo-0a1b/out/demo-binary-0a1b");
    fs::create_dir_all(target.parent().unwrap_or(&temp)).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&target, b"demo!").unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let line = DENIED_LINE.replace("{target}", &target.display().to_string());
    let program = fake_nextest(&temp, &line, 100);

    let output = run(&temp, &[&program, "nextest", "run"]);

    assert_eq!(output.status.code(), Some(100), "{output:?}");
    let text = log(&temp);
    let stat = format!("{}: mode=600 uid=", target.display());
    assert!(text.contains(&stat), "{text}");
    assert!(text.contains(" size=5 mtime="), "{text}");
    assert!(text.contains("findmnt -T"), "{text}");
}

#[test]
fn test_step_without_exec_denial_logs_no_exec_diagnostics() {
    let temp = scratch("no-denial");
    let stderr = "[double-spawn] failed to exec \"missing-binary\": No such file or directory (os error 2)\nSummary [ 0.1s] 2 tests run: 1 passed, 1 failed";
    let program = fake_nextest(&temp, stderr, 100);

    let output = run(&temp, &[&program, "nextest", "run"]);

    assert_eq!(output.status.code(), Some(100), "{output:?}");
    assert!(
        !log(&temp).contains("velnor-ci exec-failure:"),
        "{}",
        log(&temp)
    );
}
