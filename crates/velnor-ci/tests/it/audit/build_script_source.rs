// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use super::{audit, command};
use crate::io::Repo;
use velnor_test_support::real_cargo_str;

const NO_OUTPUT: &str = "build script run with a cargo fingerprint and no output";
const CLIPPY: [&str; 6] = [
    "clippy",
    "--workspace",
    "--all-targets",
    "--locked",
    "--",
    "-Dwarnings",
];
const BUILD: [&str; 4] = ["build", "--workspace", "--all-targets", "--locked"];
const TEST: [&str; 3] = ["test", "--workspace", "--locked"];

fn workspace(script: &str) -> Repo {
    let repo = Repo::new("build-script-source");
    let key = if script == "build.rs" {
        String::new()
    } else {
        format!("build = \"{script}\"\n")
    };
    repo.write_all(&[
        ("mise.toml", "[tools]\nrust = \"1.99.1\"\n"),
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"member\"]\nresolver = \"3\"\n",
        ),
        (
            "Cargo.lock",
            "version = 4\n\n[[package]]\nname = \"member\"\nversion = \"0.1.0\"\n",
        ),
        ("clippy.toml", "msrv = \"1.80\"\n"),
        (
            "member/Cargo.toml",
            &format!(
                "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n{key}"
            ),
        ),
        ("member/src/lib.rs", "pub fn member() {}\n"),
        (
            &format!("member/{script}"),
            "fn main() {\n    println!(\"cargo::rerun-if-changed=../shared/data.txt\");\n}\n",
        ),
        ("shared/data.txt", "data\n"),
    ]);
    repo
}

fn step(repo: &Repo, cargo: &[&str]) {
    let output = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .args(["inputs", "build", "--"])
        .arg(real_cargo_str())
        .args(cargo)
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    assert!(output.status.success(), "{cargo:?}: {output:?}");
}

fn build_in_order(repo: &Repo, clippy_first: bool) {
    if clippy_first {
        step(repo, &CLIPPY);
    }
    step(repo, &BUILD);
    step(repo, &TEST);
    if !clippy_first {
        step(repo, &CLIPPY);
    }
}

fn audited_lock(script: &str, clippy_first: bool) -> String {
    let repo = workspace(script);
    build_in_order(&repo, clippy_first);
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
        .unwrap_or_else(|error| unreachable!("{error}"))
}

const READS: &str = "Cargo.toml\tmember\nclippy.toml\tmember\nshared/data.txt\tmember\n";

#[test]
fn a_clippy_first_lane_audits_a_member_build_script_beside_a_clippy_config() {
    assert_eq!(audited_lock("build.rs", true), READS);
}

#[test]
fn a_build_first_lane_still_audits_a_member_build_script_beside_a_clippy_config() {
    assert!(audited_lock("build.rs", false).contains("shared/data.txt\tmember\n"));
}

#[test]
fn the_lock_lists_the_clippy_reads_whatever_the_step_order() {
    assert_eq!(audited_lock("build.rs", false), READS);
}

#[test]
fn a_clippy_first_lane_audits_a_build_script_the_manifest_names() {
    assert_eq!(audited_lock("src/gen.rs", true), READS);
}

#[test]
fn a_run_without_its_output_is_still_reported_after_a_clippy_first_lane() {
    let repo = workspace("build.rs");
    build_in_order(&repo, true);
    let outputs: Vec<PathBuf> = files(&repo.root().join("target/debug/build"))
        .into_iter()
        .filter(|file| {
            file.file_name()
                .is_some_and(|name| name == "stdout" || name == "output")
        })
        .collect();
    assert!(!outputs.is_empty(), "the build left no run output");
    for output in outputs {
        fs::remove_file(output).unwrap_or_else(|error| unreachable!("{error}"));
    }
    let refused = audit(&repo, &["--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains(NO_OUTPUT),
        "{refused:?}"
    );
}

fn files(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files(&path));
        } else {
            found.push(path);
        }
    }
    found
}
