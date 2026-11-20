// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use super::super::velnor_ci;
use super::entries::{VARIABLE, audit_within, declared_tree};

#[test]
fn a_non_unicode_entry_limit_is_refused_and_names_the_variable() {
    let repo = declared_tree(5);
    let refused = velnor_ci()
        .env(VARIABLE, OsStr::from_bytes(b"\xff"))
        .current_dir(repo.root())
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains(VARIABLE),
        "{refused:?}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_tree_above_100000_entries_passes_under_the_default_limit() {
    let repo = declared_tree(100_001);
    let within = audit_within(&repo, None);
    assert!(within.status.success(), "{within:?}");
}
