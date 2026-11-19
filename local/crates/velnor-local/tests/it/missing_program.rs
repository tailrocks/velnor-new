// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A host program that is not installed is named in the error, not reported as a bare ENOENT.

use std::process::Command;

use crate::support::temp_dir;

#[test]
fn down_without_docker_names_the_missing_program() {
    let dir = temp_dir("missing-docker");
    let empty = temp_dir("missing-docker-path");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-local"))
        .args(["down", "--dir"])
        .arg(&dir)
        .env("PATH", &empty)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("docker"), "{stderr}");
}
