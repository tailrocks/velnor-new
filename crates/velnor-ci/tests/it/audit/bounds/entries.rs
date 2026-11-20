// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Output;
use std::time::{Duration, UNIX_EPOCH};

use super::super::{inventory, own_units, unit, velnor_ci, workspace_below_the_root};
use crate::io::Repo;

const VARIABLE: &str = "VELNOR_AUDIT_MAX_ENTRIES";

/// A workspace whose build output declares `shared/first`, so the audit scans
/// that tree on every run. The tree holds `entries` empty files.
fn declared_tree(entries: usize) -> Repo {
    let repo = workspace_below_the_root();
    let directory = repo.root().join("shared/first");
    fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    let seed = directory.join("seed");
    fs::write(&seed, "").unwrap_or_else(|e| unreachable!("{e}"));
    for index in 0..entries {
        fs::hard_link(&seed, directory.join(index.to_string()))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
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

fn audit_within(repo: &Repo, limit: Option<&str>) -> Output {
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
