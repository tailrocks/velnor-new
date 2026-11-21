// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Output;
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use super::super::{inventory, own_units, unit, velnor_ci, workspace_below_the_root};
use crate::io::Repo;

pub(super) const VARIABLE: &str = "VELNOR_AUDIT_MAX_ENTRIES";

const LINKS_PER_INODE: usize = 10_000;

pub(super) fn declared_tree(entries: usize) -> Repo {
    let repo = workspace_below_the_root();
    let directory = repo.root().join("shared/first");
    fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    for seed in 0..entries.div_ceil(LINKS_PER_INODE) {
        let seed = directory.join(format!("seed-{seed}"));
        fs::write(&seed, "").unwrap_or_else(|e| unreachable!("{e}"));
    }
    link_entries(&directory, entries);
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    let output = format!("{target}/build/lib-f000000000000008/output");
    unit(
        &repo,
        &output,
        "cargo:rerun-if-changed=../../shared/first\n",
    );
    repo.set_mtime(&output, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    inventory::build(&repo, &output, None);
    repo
}

// Each hard link is a metadata round trip, so a serial loop over 100 000
// entries dominates the test. The seeds exist before any link is made, so
// the links are independent and can be created from a few threads.
fn link_entries(directory: &Path, entries: usize) {
    const THREADS: usize = 8;
    let chunk = entries.div_ceil(THREADS).max(1);
    thread::scope(|scope| {
        for start in (0..entries).step_by(chunk) {
            scope.spawn(move || {
                for index in start..(start + chunk).min(entries) {
                    let seed = directory.join(format!("seed-{}", index / LINKS_PER_INODE));
                    fs::hard_link(&seed, directory.join(index.to_string()))
                        .unwrap_or_else(|e| unreachable!("{e}"));
                }
            });
        }
    });
}

pub(super) fn audit_within(repo: &Repo, limit: Option<&str>) -> Output {
    let mut command = velnor_ci();
    command
        .env_remove(VARIABLE)
        .current_dir(repo.root())
        .args(["inputs", "audit", "--write"]);
    if let Some(limit) = limit {
        command.env(VARIABLE, limit);
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_scan_over_the_configured_entry_limit_fails_and_names_the_variable() {
    let repo = declared_tree(5);
    let refused = audit_within(&repo, Some("3"));
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let error = String::from_utf8_lossy(&refused.stderr);
    assert!(
        error.contains(VARIABLE) && error.contains("more than 3 entries"),
        "{refused:?}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_scan_within_the_configured_entry_limit_passes() {
    let repo = declared_tree(5);
    let within = audit_within(&repo, Some("1000"));
    assert!(within.status.success(), "{within:?}");
}

#[test]
fn an_entry_limit_outside_its_range_is_refused() {
    let repo = declared_tree(5);
    for value in ["0", "many", "10000001"] {
        let refused = audit_within(&repo, Some(value));
        assert_eq!(refused.status.code(), Some(1), "{value}: {refused:?}");
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains(VARIABLE),
            "{refused:?}"
        );
    }
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

fn build_within(repo: &Repo, limit: &str) -> Output {
    velnor_ci()
        .env(VARIABLE, limit)
        .current_dir(repo.root())
        .args(["inputs", "build", "--", "true"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_build_root_scan_over_the_configured_entry_limit_fails_and_names_the_variable() {
    let repo = workspace_below_the_root();
    let refused = build_within(&repo, "3");
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let error = String::from_utf8_lossy(&refused.stderr);
    assert!(
        error.contains(VARIABLE) && error.contains("more than 3 entries"),
        "{refused:?}"
    );
}
