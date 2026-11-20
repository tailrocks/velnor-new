// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use velnor_test_support::real_cargo;

use super::super::command;
use super::{InputDirectory, cargo_project};

#[test]
fn a_cross_target_created_by_cargo_at_the_source_root_does_not_hide_source_changes() {
    use std::time::{Duration, UNIX_EPOCH};

    let repo = cargo_project(InputDirectory::Repository);
    repo.write(".velnor/prepare", "");
    repo.write_unstaged("rust/target/runs.txt", "0");
    let version = command("rustc")
        .arg("-vV")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(version.status.success(), "{version:?}");
    let version = String::from_utf8(version.stdout).unwrap_or_else(|e| unreachable!("{e}"));
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap_or_else(|| unreachable!("{version}"));
    let compile = || {
        command(env!("CARGO_BIN_EXE_velnor-ci"))
            .current_dir(repo.root().join("rust"))
            .env("CARGO_TARGET_DIR", repo.root())
            .args([
                "inputs",
                "build",
                "--",
                "cargo",
                "build",
                "--workspace",
                "--all-targets",
                "--offline",
                "--target",
                host,
            ])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"))
    };
    let built = compile();
    assert!(built.status.success(), "{built:?}");
    let receipt = fs::read_dir(repo.root().join(host).join("debug/build"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .map(|entry| {
            entry
                .unwrap_or_else(|e| unreachable!("{e}"))
                .path()
                .join("output.velnor-inputs.json")
        })
        .find(|path| path.is_file())
        .unwrap_or_else(|| unreachable!("no build output receipt"));
    fs::remove_file(receipt).unwrap_or_else(|e| unreachable!("{e}"));
    let cached = compile();
    assert!(cached.status.success(), "{cached:?}");
    let artifact = format!("{host}/debug/new-artifact");
    repo.write_unstaged(&artifact, "generated");
    repo.set_mtime(&artifact, UNIX_EPOCH + Duration::from_secs(4_000_000_000));
    let check = || {
        command(env!("CARGO_BIN_EXE_velnor-ci"))
            .current_dir(repo.root())
            .env("CARGO_TARGET_DIR", repo.root())
            .args(["inputs", "audit", "--write"])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"))
    };
    let written = check();
    assert!(written.status.success(), "{written:?}");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    assert_eq!(
        fs::read_to_string(&lock).unwrap_or_else(|e| unreachable!("{e}")),
        ".\tapp\n"
    );
    fs::remove_file(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    repo.set_mtime(
        "shared/value.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    let stale = check();
    assert_eq!(stale.status.code(), Some(1), "{stale:?}");
    assert!(!lock.exists());
}

#[test]
fn an_unproven_cached_target_cannot_hide_a_source_file() {
    let repo = cargo_project(InputDirectory::Repository);
    repo.write(".velnor/prepare", "");
    repo.write_unstaged("rust/target/runs.txt", "0");
    let version = command("rustc")
        .arg("-vV")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(version.status.success(), "{version:?}");
    let version = String::from_utf8(version.stdout).unwrap_or_else(|e| unreachable!("{e}"));
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap_or_else(|| unreachable!("{version}"));
    let plain = command(real_cargo())
        .current_dir(repo.root().join("rust"))
        .env("CARGO_TARGET_DIR", repo.root())
        .args([
            "build",
            "--workspace",
            "--all-targets",
            "--offline",
            "--target",
            host,
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(plain.status.success(), "{plain:?}");
    let source = format!("{host}/source.txt");
    repo.write_unstaged(&source, "source");
    let wrapped = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root().join("rust"))
        .env("CARGO_TARGET_DIR", repo.root())
        .args([
            "inputs",
            "build",
            "--",
            "cargo",
            "build",
            "--workspace",
            "--all-targets",
            "--offline",
            "--target",
            host,
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        fs::read_to_string(repo.root().join(&source)).unwrap_or_else(|e| unreachable!("{e}")),
        "source"
    );
    let audited = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .env("CARGO_TARGET_DIR", repo.root())
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        !wrapped.status.success() || !audited.status.success(),
        "{wrapped:?} {audited:?}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}
