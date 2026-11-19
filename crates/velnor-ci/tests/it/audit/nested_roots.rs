// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use crate::io::Repo;

use super::unit;

const LOCK: &str =
    include_str!("../../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock");

fn package(repo: &Repo, directory: &str, name: &str, workspace: &str) {
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("{directory}/")
    };
    unit(
        repo,
        &format!("{prefix}Cargo.toml"),
        &format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n{workspace}"
        ),
    );
    unit(
        repo,
        &format!("{prefix}Cargo.lock"),
        &format!("version = 4\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
    );
    unit(repo, &format!("{prefix}src/lib.rs"), "");
}

/// A root Cargo workspace with a second, independent Cargo root nested below it.
fn nested() -> Repo {
    let repo = Repo::new("audit-nested");
    unit(&repo, ".gitignore", "target/\nlab/target/\n");
    unit(&repo, "mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    unit(
        &repo,
        ".velnor/rust-ci.lock",
        &LOCK.replace("version = \"0.1.81\"", "version = \"0.1.86\""),
    );
    package(&repo, "", "outer", "exclude = [\"lab\"]\n");
    package(&repo, "lab", "inner", "");
    repo.git(&["add", "--all"]);
    repo
}

/// Runs `inputs build` the way a generated lane does: the lane's own Cargo root is
/// the working directory and `CARGO_TARGET_DIR` names that root's target directory.
fn build(repo: &Repo, directory: &str, target: &str, command: &[&str]) -> Output {
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .env("CARGO_TARGET_DIR", root.join(target))
        .current_dir(root.join(directory))
        .args(["inputs", "build", "--"])
        .args(command)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn assert_built(output: &Output, root: &Path) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(!stderr.contains("cannot be told apart"), "{stderr}");
    assert!(root.exists(), "{}", root.display());
}

const CHECK: [&str; 6] = [
    "cargo",
    "check",
    "--workspace",
    "--all-targets",
    "--locked",
    "--offline",
];

#[test]
fn a_nested_root_lane_builds_with_the_nested_target_directory() {
    let repo = nested();
    let output = build(&repo, "lab", "lab/target", &CHECK);
    assert_built(&output, &repo.root().join("lab/target"));
}

#[test]
fn the_outer_root_lane_builds_with_the_outer_target_directory() {
    let repo = nested();
    let output = build(&repo, "", "target", &CHECK);
    assert_built(&output, &repo.root().join("target"));
}

#[test]
fn a_nested_root_lane_selected_by_manifest_path_builds_from_the_repository_root() {
    let repo = nested();
    let output = build(
        &repo,
        "",
        "lab/target",
        &[
            "cargo",
            "check",
            "--manifest-path",
            "lab/Cargo.toml",
            "--workspace",
            "--locked",
            "--offline",
        ],
    );
    assert_built(&output, &repo.root().join("lab/target"));
}

#[test]
fn a_target_directory_that_is_neither_roots_own_is_still_refused() {
    let repo = nested();
    let output = build(&repo, "lab", "target", &CHECK);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("cannot be told apart"), "{stderr}");
}
