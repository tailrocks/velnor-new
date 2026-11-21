// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::symlink;
use std::process::Command;

use crate::io::Repo;

#[test]
fn a_wrapper_depth_error_naming_a_wrapper_with_a_line_break_stays_on_one_line() {
    let repo = Repo::empty("crates-mirror-depth-line-break");
    let shim = repo.root().join("bin/cargo");
    std::fs::create_dir_all(repo.root().join("bin")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(env!("CARGO_BIN_EXE_velnor-ci"), &shim).unwrap_or_else(|e| unreachable!("{e}"));
    let output = Command::new(&shim)
        .arg("--version")
        .env("VELNOR_CARGO_WRAPPER", "wrap\n::stop-commands::token")
        .env("VELNOR_CARGO_DEPTH", "64")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!output.status.success(), "{output:?}");
    assert!(
        stderr
            .lines()
            .all(|line| !line.starts_with("::stop-commands::")),
        "{stderr}"
    );
    assert!(stderr.contains("%0A::stop-commands::token"), "{stderr}");
}
