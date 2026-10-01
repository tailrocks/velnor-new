//! Committed-manifest file-read plus closed-dispatch tests.
//!
//! Declared via `#[path]` from `discover.rs` under `cfg(test)` so the
//! discovery module keeps the file size gate. The helper is
//! cfg-independent, so these debug-mode tests cover the exact read the
//! release twin relies on.

use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::StackCandidate;

use super::{detected_projects, read_manifest_file};
use crate::safe_read::MAX_REPO_FILE_BYTES;

/// Present file returns its exact text.
#[test]
fn present_file_returns_its_text() {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(&dir).expect("velnor dir");
    fs::write(dir.join("release-manifest.json"), "{\"schema\":1}").expect("manifest");
    assert_eq!(
        read_manifest_file(root.path()).expect("readable"),
        Some("{\"schema\":1}".to_owned())
    );
}

/// Absent file returns `None` (release builds fail closed on this).
#[test]
fn absent_file_returns_none() {
    let root = TempDir::new().expect("temp root");
    assert_eq!(read_manifest_file(root.path()).expect("absent"), None);
    fs::create_dir_all(root.path().join(".velnor")).expect("velnor dir");
    assert_eq!(read_manifest_file(root.path()).expect("absent"), None);
}

/// Non-UTF-8 file errors instead of masking as absent (X6).
#[test]
fn non_utf8_file_errors() {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(&dir).expect("velnor dir");
    fs::write(dir.join("release-manifest.json"), [0xff, 0xfe]).expect("manifest");
    assert!(read_manifest_file(root.path()).is_err());
}

/// Symlink and directory stand-ins error instead of masking (X6).
#[test]
#[cfg(unix)]
fn symlink_file_errors() {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(&dir).expect("velnor dir");
    fs::write(dir.join("real.json"), "{\"schema\":1}").expect("target");
    std::os::unix::fs::symlink(dir.join("real.json"), dir.join("release-manifest.json"))
        .expect("symlink");
    let err = read_manifest_file(root.path()).expect_err("symlink refused");
    assert!(err.to_string().contains("symlink_refused"), "{err}");
}

/// A directory at the manifest path errors instead of masking (X6).
#[test]
fn directory_at_manifest_path_errors() {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(dir.join("release-manifest.json")).expect("dir at path");
    assert!(read_manifest_file(root.path()).is_err());
}

/// Oversize files error instead of exhausting memory (X6).
#[test]
fn oversize_file_errors() {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(&dir).expect("velnor dir");
    let big = "x".repeat(usize::try_from(MAX_REPO_FILE_BYTES + 1).expect("bound fits"));
    fs::write(dir.join("release-manifest.json"), big).expect("manifest");
    let err = read_manifest_file(root.path()).expect_err("oversize refused");
    assert!(err.to_string().contains("oversize"), "{err}");
}

/// M6 spike: candidates from unregistered stacks fail closed at dispatch.
///
/// No silent skip, no default arm: the second stack must register before
/// any candidate converts.
#[test]
fn unregistered_stack_candidate_fails_closed() {
    let candidate = StackCandidate {
        stack_id: "cobol".to_owned(),
        unit_root: String::new(),
    };
    let err = detected_projects(std::slice::from_ref(&candidate)).expect_err("must fail");
    assert!(err.to_string().contains("unregistered_stack"), "{err}");
}

/// T10: registered tofu candidates convert to detector records.
///
/// The T09 pending arm is open: conversion is real (stack, root, and
/// unit-root evidence path), never a silent skip.
#[test]
fn tofu_candidate_converts_to_project() {
    let candidate = StackCandidate {
        stack_id: "tofu".to_owned(),
        unit_root: String::new(),
    };
    let projects = detected_projects(std::slice::from_ref(&candidate)).expect("converts");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].stack_id, "tofu");
    assert_eq!(projects[0].project_root, "");
    assert_eq!(projects[0].manifest, "");
}
