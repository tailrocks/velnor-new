// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn checkout(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("velnor-ci-metadata-{}-{name}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    }
    fs::create_dir_all(dir.join("src")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(dir.join("src/lib.rs"), "").unwrap_or_else(|e| unreachable!("{e}"));
    let dir = fs::canonicalize(dir).unwrap_or_else(|e| unreachable!("{e}"));
    git(&dir, &["init", "--quiet"]);
    git(&dir, &["add", "."]);
    velnor_gen::io::write_metadata(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    git(&dir, &["add", "."]);
    dir
}

fn git(dir: &PathBuf, arguments: &[&str]) {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(status.success());
}

fn check(dir: &PathBuf) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("check-metadata")
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn check_metadata_passes_on_what_velnor_gen_wrote_and_fails_on_a_new_target() {
    let dir = checkout("drift");
    assert!(check(&dir).status.success());
    fs::create_dir_all(dir.join("tests")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(dir.join("tests/x.rs"), "").unwrap_or_else(|e| unreachable!("{e}"));
    git(&dir, &["add", "."]);
    let output = check(&dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("a file Cargo discovers as a target was added or removed"),
        "{stderr}"
    );
    fs::remove_dir_all(&dir).ok();
}
