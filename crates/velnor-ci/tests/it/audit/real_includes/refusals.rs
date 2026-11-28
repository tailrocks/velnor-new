// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

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

fn without_fingerprint_json(layout: Layout) {
    let repo = built(layout);
    assert!(remove_named(&repo.root().join("a/target"), "lib-x.json") > 0);
    refused(&repo, "has no lib-x.json");
}

#[test]
fn a_unit_whose_fingerprint_json_is_missing_refuses_the_audit_in_the_legacy_layout() {
    without_fingerprint_json(Layout::Legacy);
}

#[test]
fn a_unit_whose_fingerprint_json_is_missing_refuses_the_audit_in_the_new_layout() {
    without_fingerprint_json(Layout::New);
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

fn corrupted_fingerprint(layout: Layout) {
    let repo = built(layout);
    let mut files = Vec::new();
    named(&repo.root().join("a/target"), "lib-x.json", &mut files);
    assert_ne!(files.len(), 0);
    for file in files {
        fs::write(file, "{").unwrap_or_else(|e| unreachable!("{e}"));
    }
    refused(&repo, "lib-x.json is not valid JSON");
}

fn named(directory: &Path, name: &str, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            named(&path, name, found);
        } else if path.file_name().is_some_and(|file| file == name) {
            found.push(path);
        }
    }
}

#[test]
fn a_fingerprint_that_is_not_json_refuses_the_audit_in_the_legacy_layout() {
    corrupted_fingerprint(Layout::Legacy);
}

#[test]
fn a_fingerprint_that_is_not_json_refuses_the_audit_in_the_new_layout() {
    corrupted_fingerprint(Layout::New);
}

#[test]
fn an_unhashed_dep_info_file_beside_several_units_without_dep_info_refuses_the_audit() {
    let repo = built(Layout::Legacy);
    let deps = repo.root().join("a/target/debug/deps");
    let mut hashed = Vec::new();
    for entry in fs::read_dir(&deps).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("x-") && Path::new(&name).extension().is_some_and(|e| e == "d") {
            hashed.push(entry.path());
        }
    }
    assert_ne!(hashed.len(), 0);
    for file in hashed {
        fs::remove_file(file).unwrap_or_else(|e| unreachable!("{e}"));
    }
    fs::write(deps.join("x.d"), "x: a/x/src/lib.rs\n").unwrap_or_else(|e| unreachable!("{e}"));
    refused(
        &repo,
        "2 fingerprint units of target x have no hashed dep-info",
    );
}
