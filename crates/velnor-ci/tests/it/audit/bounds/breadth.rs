// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Output;

use super::super::{audit, own_units, unit, velnor_ci, workspace_below_the_root};
use crate::io::Repo;

const VARIABLE: &str = "VELNOR_TARGET_MAX_DIRECTORIES";

fn prepared() -> (Repo, Vec<u8>) {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(
        &repo,
        "rust/target/debug/deps/app-f000000000000001.d",
        "out.d: app/src/lib.rs app/src/../../../shared/greeting.txt\n",
    );
    let baseline = audit(&repo, &["--write"]);
    assert!(baseline.status.success(), "{baseline:?}");
    let lock = fs::read(repo.root().join(".velnor/rust-inputs.lock"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(lock, b"shared/greeting.txt\tapp\n");
    (repo, lock)
}

fn widen(repo: &Repo, count: usize) {
    let wide = repo.root().join("rust/target/breadth");
    fs::create_dir_all(&wide).unwrap_or_else(|e| unreachable!("{e}"));
    for index in 0..count {
        fs::create_dir(wide.join(index.to_string())).unwrap_or_else(|e| unreachable!("{e}"));
    }
}

fn audit_within(repo: &Repo, limit: &str) -> Output {
    velnor_ci()
        .env(VARIABLE, limit)
        .current_dir(repo.root())
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_target_directory_of_a_full_build_with_many_directories_is_audited() {
    let (repo, lock) = prepared();
    widen(&repo, 12_000);
    let audited = audit(&repo, &["--write"]);
    assert!(audited.status.success(), "{audited:?}");
    assert_eq!(
        fs::read(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        lock
    );
}

#[test]
fn a_build_command_over_a_target_directory_with_many_directories_succeeds() {
    let (repo, _) = prepared();
    widen(&repo, 12_000);
    let built = velnor_ci()
        .current_dir(repo.root().join("rust"))
        .args(["inputs", "build", "--", "true"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
}

#[test]
fn target_directory_count_is_bounded_without_removing_the_existing_lock() {
    let (repo, before) = prepared();
    let existing = directory_count(&repo.root().join("rust/target"));
    widen(&repo, 100);
    let limit = existing + 1 + 100;
    let within_limit = audit_within(&repo, &limit.to_string());
    assert!(within_limit.status.success(), "{within_limit:?}");

    fs::create_dir(repo.root().join("rust/target/breadth/over-limit"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let beyond_limit = audit_within(&repo, &limit.to_string());
    assert_eq!(beyond_limit.status.code(), Some(1), "{beyond_limit:?}");
    let error = String::from_utf8_lossy(&beyond_limit.stderr);
    assert!(
        error.contains(&format!(
            "target directory traversal exceeds {limit} directories"
        )) && error.contains(VARIABLE),
        "{error}"
    );
    assert_eq!(
        fs::read(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        before
    );
}

#[test]
fn a_directory_bound_outside_its_range_is_refused() {
    let (repo, before) = prepared();
    for value in ["0", "many", "10000001"] {
        let refused = audit_within(&repo, value);
        assert_eq!(refused.status.code(), Some(1), "{value}: {refused:?}");
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains(VARIABLE),
            "{refused:?}"
        );
    }
    assert_eq!(
        fs::read(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        before
    );
}

fn directory_count(root: &Path) -> usize {
    1 + fs::read_dir(root)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .map(|entry| entry.unwrap_or_else(|e| unreachable!("{e}")))
        .filter(|entry| {
            entry
                .file_type()
                .unwrap_or_else(|e| unreachable!("{e}"))
                .is_dir()
        })
        .map(|entry| directory_count(&entry.path()))
        .sum::<usize>()
}
