// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::PathBuf;

use super::{command, own_units, workspace_below_the_root};
use crate::io::Repo;
use velnor_test_support::real_cargo_str;

fn prepared() -> (Repo, PathBuf) {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    repo.write_unstaged(
        "driver/cargo.rs",
        r#"use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "metadata") {
        std::process::exit(std::process::Command::new("@CARGO@").args(&args).status().unwrap().code().unwrap_or(1));
    }
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(directory) => fs::write("pinned.txt", directory.to_string_lossy().as_bytes()).unwrap(),
        None => {
            let managed = std::env::var("MANAGED_TARGET").unwrap();
            fs::rename("target", &managed).unwrap();
            symlink(Path::new(&managed), "target").unwrap();
        }
    }
}
"#
        .replace("@CARGO@", &real_cargo_str())
        .as_str(),
    );
    let compiled = command("rustc")
        .current_dir(repo.root())
        .args(["--edition=2024", "driver/cargo.rs", "-o", "driver/cargo"])
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    assert!(compiled.status.success(), "{compiled:?}");
    let managed = PathBuf::from(format!("{}-managed-target", repo.root().display()));
    (repo, managed)
}

#[test]
fn a_build_leaves_the_workspace_target_a_real_directory_when_a_tool_would_relocate_it() {
    let (repo, managed) = prepared();
    let built = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root().join("rust"))
        .env_remove("CARGO_TARGET_DIR")
        .env("MANAGED_TARGET", &managed)
        .args(["inputs", "build", "--"])
        .arg(repo.root().join("driver/cargo"))
        .args(["build", "--workspace"])
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    let target = repo.root().join("rust/target");
    let kind = fs::symlink_metadata(&target).unwrap_or_else(|error| unreachable!("{error}"));
    assert!(
        kind.is_dir(),
        "target must stay a real directory: {kind:?} {built:?}"
    );
    let pinned = fs::read_to_string(repo.root().join("rust/pinned.txt"))
        .unwrap_or_else(|error| unreachable!("{error} {built:?}"));
    assert_eq!(
        fs::canonicalize(pinned).unwrap_or_else(|error| unreachable!("{error}")),
        fs::canonicalize(&target).unwrap_or_else(|error| unreachable!("{error}"))
    );
}

#[test]
fn a_target_directory_the_job_names_is_kept() {
    let (repo, managed) = prepared();
    let named = repo.root().join("named-target");
    fs::create_dir_all(&named).unwrap_or_else(|error| unreachable!("{error}"));
    let built = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root().join("rust"))
        .env("CARGO_TARGET_DIR", &named)
        .env("MANAGED_TARGET", &managed)
        .args(["inputs", "build", "--"])
        .arg(repo.root().join("driver/cargo"))
        .args(["build", "--workspace"])
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    assert_eq!(
        fs::read_to_string(repo.root().join("rust/pinned.txt"))
            .unwrap_or_else(|error| unreachable!("{error} {built:?}")),
        named.display().to_string()
    );
}
