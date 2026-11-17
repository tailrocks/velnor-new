// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Command;

use super::{audit, own_units, unit, workspace_below_the_root};

mod aggregate;
mod breadth;
mod build_output;

#[test]
fn an_excessively_deep_target_directory_returns_an_error_and_keeps_the_inputs_lock() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(
        &repo,
        "rust/target/debug/deps/app-f000000000000001.d",
        "out.d: app/src/lib.rs app/src/../../../shared/greeting.txt\n",
    );
    let baseline = audit(&repo, &["--write"]);
    assert!(baseline.status.success(), "{baseline:?}");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    let before = fs::read(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(before, b"shared/greeting.txt\tapp\n");
    let mut directory = repo.root().join("rust/target");
    for _ in 0..65 {
        directory.push("d");
        fs::create_dir(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    }
    let output = Command::new("sh")
        .args([
            "-c",
            "ulimit -n 1024 && exec \"$1\" inputs audit --write",
            "audit-depth",
            env!("CARGO_BIN_EXE_velnor-ci"),
        ])
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("target directory depth exceeds 64"),
        "{output:?}"
    );
    assert_eq!(fs::read(lock).unwrap_or_else(|e| unreachable!("{e}")), before);
}

#[test]
fn dep_info_over_the_source_limit_returns_an_error_and_keeps_the_inputs_lock() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(
        &repo,
        "rust/target/debug/deps/extra-f000000000000001.d",
        "out.d: app/src/lib.rs app/src/../../../shared/greeting.txt\n",
    );
    let dep_info = "rust/target/debug/deps/app-f000000000000001.d";
    let mut text = "out.d: app/src/lib.rs\n".to_owned();
    text.push_str(&" ".repeat((1 << 20) - text.len()));
    unit(&repo, dep_info, &text);
    let baseline = audit(&repo, &["--write"]);
    assert!(baseline.status.success(), "{baseline:?}");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    let before = fs::read(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(before, b"shared/greeting.txt\tapp\n");
    text.push(' ');
    unit(&repo, dep_info, &text);
    let output = audit(&repo, &["--write"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains(dep_info), "{output:?}");
    assert!(error.contains("larger than 1048576 bytes"), "{output:?}");
    assert_eq!(fs::read(lock).unwrap_or_else(|e| unreachable!("{e}")), before);
}
