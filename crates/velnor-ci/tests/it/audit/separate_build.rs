// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Output;
use std::time::{Duration, UNIX_EPOCH};

use super::new_layout::own_units_1100;
use super::{unit, velnor_ci, workspace_below_the_root};
use crate::io::Repo;

const APART: &str = "rust/build";
const NESTED: &str = "rust/target/bd";

fn output(directory: &str) -> String {
    format!("{directory}/debug/build/lib/f000000000000008/run/stdout")
}

fn with_build_directory(repo: &Repo, directory: &str, arguments: &[&str]) -> Output {
    velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_BUILD_BUILD_DIR", repo.root().join(directory))
        .args(arguments)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn built_in(directory: &str, left_out: &[&str]) -> Repo {
    let repo = workspace_below_the_root();
    own_units_1100(&repo, &format!("{directory}/debug"), left_out);
    unit(&repo, "rust/target/debug/app.d", "app: app/src/lib.rs\n");
    unit(&repo, "rust/target/debug/libapp.rlib", "");
    unit(&repo, &output(directory), "cargo:rerun-if-changed=../..\n");
    repo
}

fn stamp(repo: &Repo, directory: &str) {
    let built = with_build_directory(
        repo,
        directory,
        &[
            "inputs",
            "build",
            "--",
            "touch",
            "-m",
            "-t",
            "206501010000",
            &output(directory),
        ],
    );
    assert!(built.status.success(), "{built:?}");
}

fn lock(repo: &Repo) -> String {
    fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn units_in_a_build_directory_apart_from_the_target_directory_are_audited() {
    let repo = built_in(APART, &[]);
    stamp(&repo, APART);
    let written = with_build_directory(&repo, APART, &["inputs", "audit", "--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(lock(&repo), ".\tlib\n");
}

#[test]
fn a_file_written_into_the_build_directory_after_the_build_does_not_make_the_inputs_stale() {
    let repo = built_in(APART, &[]);
    stamp(&repo, APART);
    unit(&repo, "rust/build/debug/late.o", "");
    repo.set_mtime(
        "rust/build/debug/late.o",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    let written = with_build_directory(&repo, APART, &["inputs", "audit", "--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(lock(&repo), ".\tlib\n");
}

#[test]
fn a_unit_without_dep_info_in_a_separate_build_directory_makes_the_audit_refuse() {
    let repo = built_in(APART, &["lib"]);
    let refused = with_build_directory(&repo, APART, &["inputs", "audit", "--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("found no current dep-info in") && stderr.contains("lib"),
        "{stderr}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_build_directory_that_holds_no_units_while_the_target_holds_outputs_makes_the_audit_refuse() {
    let repo = workspace_below_the_root();
    unit(&repo, "rust/target/debug/app.d", "app: app/src/lib.rs\n");
    unit(&repo, "rust/target/debug/libapp.rlib", "");
    fs::create_dir_all(repo.root().join("rust/build")).unwrap_or_else(|e| unreachable!("{e}"));
    let refused = with_build_directory(&repo, APART, &["inputs", "audit", "--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("found no units in") && stderr.contains("rust/build"),
        "{stderr}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

fn unit_count(output: &Output) -> usize {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let (_, tail) = stdout
        .split_once(" reads from ")
        .unwrap_or_else(|| unreachable!("{stdout}"));
    tail.split_whitespace()
        .next()
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| unreachable!("{stdout}"))
}

#[test]
fn a_build_directory_inside_the_target_directory_counts_each_unit_once() {
    let apart = built_in(APART, &[]);
    stamp(&apart, APART);
    let separate = with_build_directory(&apart, APART, &["inputs", "audit", "--write"]);
    assert!(separate.status.success(), "{separate:?}");

    let nested = built_in(NESTED, &[]);
    stamp(&nested, NESTED);
    let inside = with_build_directory(&nested, NESTED, &["inputs", "audit", "--write"]);
    assert!(inside.status.success(), "{inside:?}");
    assert_eq!(unit_count(&inside), unit_count(&separate));
    assert_eq!(lock(&nested), ".\tlib\n");
}
