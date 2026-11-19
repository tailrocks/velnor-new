// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;
use std::fs::{self, File, FileTimes};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use super::{audit, own_units, unit, workspace_below_the_root};
use crate::io::Repo;

const OUTPUT: &str = "rust/target/debug/build/lib-f000000000000008/output";

pub(super) fn build(repo: &Repo, output: &str, target: Option<&Path>) {
    let mut command = super::velnor_ci();
    command.current_dir(repo.root());
    if let Some(target) = target {
        command.env("CARGO_TARGET_DIR", target);
    }
    let built = command
        .args([
            "inputs",
            "build",
            "--",
            "touch",
            "-m",
            "-t",
            "206501010000",
            output,
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
}

fn declared_repository() -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../..\n");
    build(&repo, OUTPUT, None);
    repo
}

fn assert_stale(repo: &Repo) {
    let refused = audit(repo, &["--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

fn old_directory(repo: &Repo, path: &str) {
    File::open(repo.root().join(path))
        .and_then(|file| {
            file.set_times(
                FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1_000_000_000)),
            )
        })
        .unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn deleting_a_sibling_of_the_excluded_target_is_stale() {
    let repo = declared_repository();
    fs::remove_file(repo.root().join("shared/greeting.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    old_directory(&repo, "shared");
    assert_stale(&repo);
}

#[test]
fn adding_a_sibling_of_the_excluded_target_is_stale() {
    let repo = declared_repository();
    unit(&repo, "shared/new.txt", "new");
    repo.set_mtime(
        "shared/new.txt",
        UNIX_EPOCH + Duration::from_secs(1_000_000_000),
    );
    old_directory(&repo, "shared");
    assert_stale(&repo);
}

#[test]
fn a_declared_directory_without_a_prebuild_inventory_is_stale() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../..\n");
    repo.set_mtime(OUTPUT, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    let unchanged = super::velnor_ci()
        .current_dir(repo.root())
        .args(["inputs", "build", "--", "true"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(unchanged.status.success(), "{unchanged:?}");
    assert_stale(&repo);
}

#[test]
fn creating_only_an_excluded_target_keeps_the_inventory_fresh() {
    let repo = declared_repository();
    unit(&repo, "rust/target/new-artifact", "generated");
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        ".\tlib\n"
    );
}

#[test]
fn a_source_root_used_as_the_target_excludes_its_generated_artifacts() {
    let repo = workspace_below_the_root();
    own_units(&repo, "debug", &[]);
    let output = "debug/build/lib-f000000000000008/output";
    unit(&repo, output, "cargo:rerun-if-changed=../..\n");
    build(&repo, output, Some(repo.root()));
    unit(&repo, "debug/new-artifact", "generated");
    repo.set_mtime(
        "debug/new-artifact",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    let written = super::velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_TARGET_DIR", repo.root())
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        ".\tlib\n"
    );
}

fn named_files() -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    fs::create_dir_all(repo.root().join("shared/many")).unwrap_or_else(|e| unreachable!("{e}"));
    unit(&repo, "shared/many/seed", "");
    repo.set_mtime(
        "shared/many/seed",
        UNIX_EPOCH + Duration::from_secs(1_000_000_000),
    );
    let mut output = String::new();
    for index in 0..16 {
        unit(&repo, &format!("shared/many/{index}"), "");
        repo.set_mtime(
            &format!("shared/many/{index}"),
            UNIX_EPOCH + Duration::from_secs(1_000_000_000),
        );
        writeln!(output, "cargo:rerun-if-changed=../../shared/many/{index}")
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    unit(&repo, OUTPUT, &output);
    build(&repo, OUTPUT, None);
    repo
}

fn named_files_lock(repo: &Repo) {
    let mut expected: Vec<_> = (0..16)
        .map(|index| format!("shared/many/{index}\tlib\n"))
        .collect();
    expected.sort();
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        expected.concat()
    );
}

#[test]
fn directly_named_files_are_fresh_without_a_directory_scan() {
    let repo = named_files();
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    named_files_lock(&repo);
}

#[test]
fn directly_named_files_do_not_consume_the_directory_walk_budget() {
    let repo = named_files();
    let written = velnor_ci::io::audit_inputs(
        repo.root(),
        &velnor_ci::io::AuditOptions {
            write: true,
            root: None,
            max_directory_entries: 8,
            inherit_target_dir: false,
        },
    );
    assert!(written.is_ok(), "{written:?}");
    named_files_lock(&repo);
}

#[test]
fn a_first_build_captures_inventory_before_its_target_exists() {
    let repo = workspace_below_the_root();
    own_units(&repo, "staged/debug", &[]);
    unit(
        &repo,
        "staged/debug/build/lib-f000000000000008/output",
        "cargo:rerun-if-changed=../../shared\n",
    );
    assert!(!repo.root().join("rust/target").exists());
    let built = super::velnor_ci().current_dir(repo.root()).args(["inputs", "build", "--", "sh", "-c", "mkdir -p rust/target; mv staged/debug rust/target/debug; touch -m -t 206501010000 rust/target/debug/build/lib-f000000000000008/output"]).output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        "shared\tlib\n"
    );
}

#[test]
fn a_failed_build_cannot_attach_inventory_to_its_partial_output() {
    let repo = declared_repository();
    let failed = super::velnor_ci()
        .current_dir(repo.root())
        .args([
            "inputs",
            "build",
            "--",
            "sh",
            "-c",
            "touch -m -t 206601010000 rust/target/debug/build/lib-f000000000000008/output; exit 1",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(failed.status.code(), Some(1), "{failed:?}");
    assert!(
        !repo
            .root()
            .join(format!("{OUTPUT}.velnor-inputs.json"))
            .exists()
    );
    assert_stale(&repo);
}

#[test]
fn output_content_changes_cannot_reuse_an_inventory_with_the_same_mtime() {
    let repo = declared_repository();
    let modified = repo.mtime(OUTPUT);
    unit(
        &repo,
        OUTPUT,
        "cargo:rerun-if-changed=../..\ncargo:rustc-env=CHANGED=true\n",
    );
    repo.set_mtime(OUTPUT, modified);
    assert_stale(&repo);
}

#[test]
fn touching_unchanged_output_cannot_reuse_its_previous_inventory() {
    let repo = declared_repository();
    repo.set_mtime(OUTPUT, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    assert_stale(&repo);
}

#[test]
fn membership_changes_during_the_build_do_not_become_the_prebuild_inventory() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../../shared\n");
    let built = super::velnor_ci()
        .current_dir(repo.root())
        .args([
            "inputs", "build", "--", "sh", "-c",
            "printf '' > shared/new.txt; touch -m -t 200101010000 shared/new.txt; touch -m -t 206501010000 rust/target/debug/build/lib-f000000000000008/output",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
    assert_stale(&repo);
}

#[test]
fn a_missing_untracked_directory_without_inventory_is_stale() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, "shared/generated/value.txt", "input");
    unit(
        &repo,
        OUTPUT,
        "cargo:rerun-if-changed=../../shared/generated\n",
    );
    fs::remove_file(repo.root().join("shared/generated/value.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir(repo.root().join("shared/generated")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.set_mtime(OUTPUT, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    assert_stale(&repo);
}

#[test]
fn replacing_a_declared_directory_with_an_older_file_is_stale() {
    for keep_inventory in [true, false] {
        let repo = workspace_below_the_root();
        own_units(&repo, "rust/target/debug", &[]);
        unit(&repo, "shared/generated/value.txt", "input");
        unit(
            &repo,
            OUTPUT,
            "cargo:rerun-if-changed=../../shared/generated\n",
        );
        build(&repo, OUTPUT, None);
        fs::remove_file(repo.root().join("shared/generated/value.txt"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::remove_dir(repo.root().join("shared/generated"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        unit(&repo, "shared/generated", "replacement");
        repo.set_mtime(
            "shared/generated",
            UNIX_EPOCH + Duration::from_secs(1_000_000_000),
        );
        if !keep_inventory {
            fs::remove_file(repo.root().join(format!("{OUTPUT}.velnor-inputs.json")))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        assert_stale(&repo);
    }
}
