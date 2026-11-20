// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};

use crate::io::Repo;

const LOCK: &str =
    include_str!("../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock");

fn helper(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(args)
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn success(root: &Path, args: &[&str]) {
    let output = helper(root, args);
    assert!(output.status.success(), "{args:?}: {output:?}");
}

fn checkout(pin: &str) -> Repo {
    let repo = Repo::new("nested-targets");
    repo.write_all(&[
        ("mise.toml", "[tools]\nrust='1.99.1'\n"),
        (".gitignore", "/target/\n**/target/\n"),
        (
            "Cargo.toml",
            "[package]\nname='top'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("src/lib.rs", "pub fn top() {}\n"),
        (
            "tool/Cargo.toml",
            "[package]\nname='tool'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("tool/src/lib.rs", "pub fn tool() {}\n"),
        (".velnor/rust-ci.lock", &LOCK.replace("0.1.81", pin)),
    ]);
    for directory in [".", "tool"] {
        let output = Command::new(velnor_test_support::real_cargo())
            .args(["generate-lockfile", "--offline"])
            .current_dir(repo.root().join(directory))
            .env_remove("CARGO_TARGET_DIR")
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{output:?}");
    }
    repo.git(&["add", "--all"]);
    velnor_gen::io::write_metadata(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    repo
}

/// The cold cleanup finds every root's target through the recorded repository, with the same rule
/// the generator used to write the workflow, and a target that lies below another root's target
/// is removed without an error.
fn cold_cleanup_removes(pin: &str, nested: &str, relative: &str) {
    let repo = checkout(pin);
    success(
        &repo.root().join("tool"),
        &["inputs", "save", "--target-dir", relative],
    );
    success(
        &repo.root().join("tool"),
        &["inputs", "restore", "--target-dir", relative],
    );
    repo.write_unstaged(&format!("{nested}/stale"), "cache\n");
    repo.write_unstaged("target/stale", "cache\n");
    repo.set_mtime("src/lib.rs", UNIX_EPOCH - Duration::from_secs(1));
    let output = helper(repo.root(), &["restored"]);
    assert!(output.status.success(), "{output:?}");
    assert!(!repo.root().join(nested).exists(), "{pin}: {nested}");
    assert!(!repo.root().join("target/stale").exists(), "{pin}");
}

#[test]
fn the_cold_cleanup_removes_a_nested_roots_target_below_the_repository_target_folder() {
    cold_cleanup_removes(
        "0.1.107",
        "target/velnor-roots/tool",
        "../target/velnor-roots/tool",
    );
}

#[test]
fn a_helper_lock_before_the_nested_rule_still_cleans_the_folder_beside_the_manifest() {
    cold_cleanup_removes("0.1.106", "tool/target", "target");
}
