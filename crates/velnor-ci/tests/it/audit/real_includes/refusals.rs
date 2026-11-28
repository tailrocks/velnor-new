// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::oracle::{Layout, Share, build_in};
use super::two_roots::{audit_after, remove_named, repo, with_script};
use crate::io::Repo;

fn refused(repo: &Repo, needle: &str) {
    let written = audit_after(repo);
    let stderr = String::from_utf8_lossy(&written.stderr);
    assert!(!written.status.success(), "{stderr}");
    assert!(stderr.contains(needle), "{stderr}");
}

fn built(layout: Layout) -> Repo {
    let repo = repo("audit-dep-info-refusal");
    with_script(&repo, "a", "x", "", "");
    repo.git(&["add", "--all"]);
    build_in(&repo, "a", Share::Zero, layout);
    repo
}

fn without_fingerprint(layout: Layout) {
    let repo = built(layout);
    assert!(remove_named(&repo.root().join("a/target"), "lib-x") > 0);
    refused(&repo, "belongs to a unit with no fingerprint");
}

fn without_hash(layout: Layout) {
    let repo = built(layout);
    let deps = repo.root().join("a/target/debug/deps");
    fs::create_dir_all(&deps).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(deps.join("stray.d"), "stray: a/x/src/lib.rs\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    refused(&repo, "carries no unit hash");
}

#[test]
fn a_dep_info_file_with_a_hash_but_no_fingerprint_refuses_the_audit_in_the_legacy_layout() {
    without_fingerprint(Layout::Legacy);
}

#[test]
fn a_dep_info_file_with_a_hash_but_no_fingerprint_refuses_the_audit_in_the_new_layout() {
    without_fingerprint(Layout::New);
}

#[test]
fn a_dep_info_file_without_a_unit_hash_refuses_the_audit_in_the_legacy_layout() {
    without_hash(Layout::Legacy);
}

#[test]
fn a_dep_info_file_without_a_unit_hash_refuses_the_audit_in_the_new_layout() {
    without_hash(Layout::New);
}
