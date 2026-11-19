// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::time::{Duration, UNIX_EPOCH};

use super::{audit, inventory::build, own_units, unit, workspace_below_the_root};
use crate::io::Repo;

const OUTPUT: &str = "rust/target/debug/build/lib-f000000000000008/output";

fn declared_shared() -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../../shared\n");
    repo
}

fn assert_fresh(repo: &Repo, expected: &str) {
    let written = audit(repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    assert_eq!(
        fs::read_to_string(&lock).unwrap_or_else(|e| unreachable!("{e}")),
        expected
    );
    fs::remove_file(lock).unwrap_or_else(|e| unreachable!("{e}"));
}

fn assert_stale(repo: &Repo) {
    let refused = audit(repo, &["--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn an_unchanged_dangling_link_is_fresh_but_replacing_or_removing_it_is_stale() {
    for replacement in [None, Some("another-missing-file")] {
        let repo = declared_shared();
        let link = repo.root().join("shared/dangling");
        symlink("missing-file", &link).unwrap_or_else(|e| unreachable!("{e}"));
        build(&repo, OUTPUT, None);
        assert_fresh(&repo, "shared\tlib\n");
        fs::remove_file(&link).unwrap_or_else(|e| unreachable!("{e}"));
        if let Some(target) = replacement {
            symlink(target, &link).unwrap_or_else(|e| unreachable!("{e}"));
        }
        assert_stale(&repo);
    }
}

#[test]
fn independent_aliases_keep_their_descendants_and_detect_changes() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, "shared/real/nested/value.txt", "original");
    for parent in ["first", "second"] {
        fs::create_dir_all(repo.root().join(format!("shared/{parent}")))
            .unwrap_or_else(|e| unreachable!("{e}"));
        symlink(
            "../real",
            repo.root().join(format!("shared/{parent}/alias")),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
    }
    unit(
        &repo,
        OUTPUT,
        "cargo:rerun-if-changed=../../shared/first\ncargo:rerun-if-changed=../../shared/second\n",
    );
    build(&repo, OUTPUT, None);
    assert_fresh(&repo, "shared/first\tlib\nshared/second\tlib\n");
    repo.set_mtime(
        "shared/real/nested/value.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    assert_stale(&repo);
}

#[test]
fn a_link_cycle_to_the_repository_remains_finite_and_detects_new_entries() {
    let repo = declared_shared();
    fs::create_dir_all(repo.root().join(".velnor")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink("..", repo.root().join("shared/cycle")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink("loop-b", repo.root().join("shared/loop-a")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink("loop-a", repo.root().join("shared/loop-b")).unwrap_or_else(|e| unreachable!("{e}"));
    build(&repo, OUTPUT, None);
    assert_fresh(&repo, "shared\tlib\n");
    unit(&repo, "shared/new.txt", "new");
    repo.set_mtime(
        "shared/new.txt",
        UNIX_EPOCH + Duration::from_secs(1_000_000_000),
    );
    assert_stale(&repo);
}

#[test]
fn retargeting_a_declared_alias_cannot_reuse_another_directories_inventory() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    for directory in ["first", "second"] {
        unit(&repo, &format!("shared/{directory}/value.txt"), "same");
        repo.set_mtime(
            &format!("shared/{directory}/value.txt"),
            UNIX_EPOCH + Duration::from_secs(1_000_000_000),
        );
    }
    let alias = repo.root().join("shared/declared");
    symlink("first", &alias).unwrap_or_else(|e| unreachable!("{e}"));
    unit(
        &repo,
        OUTPUT,
        "cargo:rerun-if-changed=../../shared/declared\n",
    );
    build(&repo, OUTPUT, None);
    fs::remove_file(&alias).unwrap_or_else(|e| unreachable!("{e}"));
    symlink("second", &alias).unwrap_or_else(|e| unreachable!("{e}"));
    assert_stale(&repo);
}

#[test]
fn an_alias_into_the_target_does_not_include_generated_artifacts() {
    let repo = declared_shared();
    symlink("../rust/target", repo.root().join("shared/generated"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    build(&repo, OUTPUT, None);
    unit(&repo, "rust/target/new-artifact", "generated");
    repo.set_mtime(
        "rust/target/new-artifact",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    assert_fresh(&repo, "shared\tlib\n");
}

#[test]
fn an_explicit_generated_directory_is_audited_with_its_own_inventory() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, "rust/target/generated/value.txt", "original");
    unit(
        &repo,
        OUTPUT,
        "cargo:rerun-if-changed=../target/generated\ncargo:rerun-if-changed=../target/../target/generated\n",
    );
    let built = super::velnor_ci()
        .current_dir(repo.root())
        .args([
            "inputs",
            "build",
            "--directory",
            "rust/target/generated",
            "--",
            "touch",
            "-m",
            "-t",
            "206501010000",
            OUTPUT,
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
    assert_fresh(&repo, "");
    repo.set_mtime(
        "rust/target/generated/value.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    assert_stale(&repo);
}

#[test]
fn a_tracked_alias_cannot_hide_inputs_in_the_excluded_target() {
    let repo = declared_shared();
    symlink("../rust/target", repo.root().join("shared/generated"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "shared/generated"]);
    build(&repo, OUTPUT, None);
    unit(&repo, "rust/target/new-artifact", "generated");
    repo.set_mtime(
        "rust/target/new-artifact",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    assert_stale(&repo);
}

#[test]
fn parent_components_after_an_alias_resolve_in_the_linked_directory() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    repo.write("shared/real/inputs/value.txt", "actual");
    repo.write("shared/inputs/value.txt", "decoy");
    fs::create_dir_all(repo.root().join("shared/real/nested"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    symlink("real/nested", repo.root().join("shared/alias"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    unit(
        &repo,
        OUTPUT,
        "cargo:rerun-if-changed=../../shared/alias/../inputs\n",
    );
    build(&repo, OUTPUT, None);
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    let reads = fs::read_to_string(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        reads.lines().any(|line| line == "shared/real/inputs\tlib"),
        "{reads}"
    );
    fs::remove_file(lock).unwrap_or_else(|e| unreachable!("{e}"));
    repo.set_mtime(
        "shared/real/inputs/value.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    assert_stale(&repo);
}

#[test]
fn a_parent_component_cannot_make_a_saved_file_a_directory() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, "shared/file", "file");
    unit(&repo, "shared/inputs/value.txt", "input");
    symlink("file", repo.root().join("shared/alias")).unwrap_or_else(|e| unreachable!("{e}"));
    unit(
        &repo,
        OUTPUT,
        "cargo:rerun-if-changed=../../shared/alias/../inputs\n",
    );
    build(&repo, OUTPUT, None);
    fs::remove_file(repo.root().join("shared/file")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir(repo.root().join("shared/file")).unwrap_or_else(|e| unreachable!("{e}"));
    assert_stale(&repo);
}

#[test]
fn a_live_link_with_parent_components_cannot_disguise_source_as_target() {
    let repo = workspace_below_the_root();
    let target = repo.root().join("shared/target");
    own_units(&repo, "shared/target/debug", &[]);
    unit(&repo, "real/target/value.txt", "source");
    fs::create_dir_all(repo.root().join("real/nested")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink("../real/nested", repo.root().join("shared/bridge"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    symlink("bridge/../target", repo.root().join("shared/alias"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let output = "shared/target/debug/build/lib-f000000000000008/output";
    unit(&repo, output, "cargo:rerun-if-changed=../../shared\n");
    build(&repo, output, Some(&target));
    let run = || {
        super::velnor_ci()
            .current_dir(repo.root())
            .env("CARGO_TARGET_DIR", &target)
            .args(["inputs", "audit", "--write"])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"))
    };
    let written = run();
    assert!(written.status.success(), "{written:?}");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    fs::remove_file(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    repo.set_mtime(
        "real/target/value.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    let refused = run();
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(!lock.exists());
}
