// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use crate::io::Repo;

fn setup() -> Repo {
    let repo = Repo::new("cache-safety");
    repo.write("Cargo.lock", "version = 4\n");
    repo.write_unstaged("target/debug/incremental/app/data", "cached");
    repo.write_unstaged("temp/velnor-run.log", "   Compiling app v0.1.0 (/app)\n");
    repo
}

fn decide(repo: &Repo) -> (Output, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["cache", "decide", "--target-dir", "target"])
        .env("RUNNER_TEMP", repo.root().join("temp"))
        .env("GITHUB_OUTPUT", repo.root().join("output"))
        .env("CARGO_HOME", repo.root().join("cargo-home"))
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let written = fs::read_to_string(repo.root().join("output")).unwrap_or_default();
    (output, written)
}

#[test]
fn runtime_source_configuration_is_checked_before_saving() {
    for path in [
        ".cargo/config.toml",
        ".cargo/config",
        "cargo-home/config.toml",
    ] {
        let repo = setup();
        repo.write_unstaged(path, "[source.crates-io]\nreplace-with = 'private'\n[source.private]\nregistry = 'https://private.invalid/index'\n");
        let (output, written) = decide(&repo);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(written, "save=false\nsources=false\n", "{path}: {output:?}");
        assert!(repo.root().join("target/debug/incremental").is_dir());
    }
}

#[test]
fn unused_private_patch_is_not_saved() {
    let repo = setup();
    repo.write("Cargo.lock", "version = 4\n[[patch.unused]]\nname = 'secret'\nversion = '1.0.0'\nsource = 'git+https://private.invalid/secret#123'\n");
    let (output, written) = decide(&repo);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=false\nsources=false\n", "{output:?}");
}

#[test]
fn unrelated_fixture_lock_and_config_do_not_disable_the_real_root() {
    let repo = setup();
    repo.write("fixtures/Cargo.lock", "version = 4\n[[package]]\nname = 'secret'\nversion = '1.0.0'\nsource = 'git+https://private.invalid/secret#123'\n");
    repo.write(
        "fixtures/.cargo/config.toml",
        "[source.private]\nregistry = 'https://private.invalid/index'\n",
    );
    let (output, written) = decide(&repo);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n", "{output:?}");
}

#[test]
fn a_warm_target_can_save_safe_sources_after_a_source_cache_miss() {
    let repo = setup();
    repo.write_unstaged(
        "temp/velnor-run.log",
        "    Finished `dev` profile in 0.1s\n",
    );
    let (output, written) = decide(&repo);
    assert!(output.status.success(), "{output:?}");
    assert!(written.contains("save=false\n"), "{written}");
    assert!(written.contains("sources=true\n"), "{written}");
}

#[test]
fn an_unreadable_target_fails_without_save_authorization() {
    let repo = setup();
    fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.write_unstaged("target", "not a directory");
    let (output, written) = decide(&repo);
    assert!(!output.status.success(), "{output:?}");
    assert!(written.is_empty(), "{written}");
}

#[test]
fn a_symlinked_incremental_directory_fails_without_save_authorization() {
    let repo = setup();
    fs::remove_dir_all(repo.root().join("target/debug/incremental"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.write_unstaged("outside/data", "private");
    std::os::unix::fs::symlink(
        repo.root().join("outside"),
        repo.root().join("target/debug/incremental"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let (output, written) = decide(&repo);
    assert!(!output.status.success(), "{output:?}");
    assert!(written.is_empty(), "{written}");
    assert!(repo.root().join("outside/data").is_file());
}

#[test]
fn configured_target_is_expired_and_removed_on_restoration_failure() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    for invalid_path in [false, true] {
        let repo = setup();
        repo.write_unstaged("build-cache/debug/deps/app", "old");
        repo.write_unstaged("build-cache/.velnor/born", "2000-01-01\n");
        if invalid_path {
            let blob = Command::new("git")
                .args(["hash-object", "-w", "--stdin"])
                .stdin(std::process::Stdio::null())
                .current_dir(repo.root())
                .output()
                .unwrap_or_else(|e| unreachable!("{e}"));
            let blob = String::from_utf8_lossy(&blob.stdout).trim().to_owned();
            let mut entry = format!("100644,{blob},").into_bytes();
            entry.extend_from_slice(b"non-utf8-\xff");
            let status = Command::new("git")
                .args(["update-index", "--add", "--cacheinfo"])
                .arg(OsStr::from_bytes(&entry))
                .current_dir(repo.root())
                .status()
                .unwrap_or_else(|e| unreachable!("{e}"));
            assert!(status.success());
        }
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["restored", "--target-dir", "build-cache"])
            .env(
                "VELNOR_BASE_MAX_AGE_DAYS",
                if invalid_path { "0" } else { "7" },
            )
            .env_remove("GITHUB_OUTPUT")
            .current_dir(repo.root())
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{output:?}");
        assert!(!repo.root().join("build-cache").exists(), "{output:?}");
        assert!(repo.root().join("Cargo.lock").is_file());
    }
}
