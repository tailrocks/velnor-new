//! Committed-manifest file-read plus closed-dispatch tests.
//!
//! Declared via `#[path]` from `discover.rs` under `cfg(test)` because it
//! tests private detector conversion. Consumer admission returns the
//! committed file, or a debug-only stand-in when the file is absent.

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::config::{NextestTestSelection, NextestTestSelectionMode};
use velnor_actions_contract::{StackCandidate, WorkflowPolicy};
use velnor_actions_rust::{
    CompileDriver, NextestProfile, PackageRecord, TargetRecord, TaskGroup, TaskKind, TestRunner,
    VelnorV1TaskOwner,
};

use super::{
    consumer_manifest::{for_policy, read_manifest_file},
    detected_projects, propose_rust_groups, validate_selector_targets,
};
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
    assert_eq!(
        for_policy(root.path(), WorkflowPolicy::ConsumerV1).expect("consumer reads manifest"),
        (Some("{\"schema\":1}".to_owned()), false)
    );
}

/// Absent file: raw read is `None`; admission stand-in/fail-closed by build.
#[test]
fn absent_file_returns_none() {
    let root = TempDir::new().expect("temp root");
    assert_eq!(read_manifest_file(root.path()).expect("absent"), None);
    assert_absent_consumer_admission(root.path());
    fs::create_dir_all(root.path().join(".velnor")).expect("velnor dir");
    assert_eq!(read_manifest_file(root.path()).expect("absent"), None);
    assert_absent_consumer_admission(root.path());
}

/// Debug admits the stand-in; release fails closed with `None`.
#[cfg(debug_assertions)]
fn assert_absent_consumer_admission(root: &Path) {
    let (manifest, stand_in) =
        for_policy(root, WorkflowPolicy::ConsumerV1).expect("consumer stand-in");
    assert!(stand_in, "absent file must flag the debug stand-in");
    assert!(manifest.is_some(), "debug stand-in carries manifest text");
}

/// Debug admits the stand-in; release fails closed with `None`.
#[cfg(not(debug_assertions))]
fn assert_absent_consumer_admission(root: &Path) {
    assert_eq!(
        for_policy(root, WorkflowPolicy::ConsumerV1).expect("consumer absent"),
        (None, false)
    );
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

/// Velnor policy does not read irrelevant consumer-manifest inputs.
#[test]
fn velnor_policy_skips_consumer_manifest_input() {
    let root = TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(&dir).expect("velnor dir");
    let manifest = dir.join("release-manifest.json");

    fs::write(&manifest, "not json").expect("malformed manifest");
    assert_velnor_policy_skips(root.path());

    fs::remove_file(&manifest).expect("remove malformed manifest");
    fs::create_dir(&manifest).expect("directory at manifest path");
    assert_velnor_policy_skips(root.path());

    fs::remove_dir(&manifest).expect("remove manifest directory");
    fs::write(&manifest, [0xff, 0xfe]).expect("invalid UTF-8 manifest");
    assert_velnor_policy_skips(root.path());

    #[cfg(unix)]
    {
        fs::remove_file(&manifest).expect("remove invalid UTF-8 manifest");
        let target = dir.join("real-manifest.json");
        fs::write(&target, "{}\n").expect("symlink target");
        std::os::unix::fs::symlink(&target, &manifest).expect("manifest symlink");
        assert_velnor_policy_skips(root.path());
    }
}

/// Assert Velnor discovery carries no consumer manifest or stand-in.
fn assert_velnor_policy_skips(root: &Path) {
    let (manifest, stand_in) = for_policy(root, WorkflowPolicy::VelnorRepositoryV1)
        .expect("Velnor policy ignores consumer manifest input");
    assert_eq!(manifest, None);
    assert!(!stand_in);
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

fn selector() -> NextestTestSelection {
    NextestTestSelection {
        package: "demo".to_owned(),
        binary: "control_states".to_owned(),
        mode: NextestTestSelectionMode::IgnoredOnly,
        test_ids: vec!["a_test".to_owned()],
    }
}

fn task_group(package: &str, kind: TaskKind) -> TaskGroup {
    TaskGroup {
        task_id: format!("stack/rust/{package}/{}/{package}-default", kind.as_str()),
        package_id: package.to_owned(),
        package_name: package.to_owned(),
        manifest_key: "root".to_owned(),
        kind,
        configuration: "default".to_owned(),
        features: vec!["default".to_owned()],
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: if kind == TaskKind::Nextest {
            TestRunner::CargoNextest
        } else {
            TestRunner::CargoTest
        },
        nextest_profile: NextestProfile::Default,
        run_ignored: Some("all".to_owned()),
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    }
}

#[test]
fn exact_selector_changes_only_matching_nextest_group() {
    let groups = [
        task_group("demo", TaskKind::Nextest),
        task_group("demo", TaskKind::Build),
        task_group("other", TaskKind::Nextest),
    ];
    let proposals = propose_rust_groups(&groups, &[selector()]).expect("route selector");
    assert!(
        proposals[0]
            .payload
            .iter()
            .any(|arg| arg.to_str() == Some("--filterset"))
    );
    assert_eq!(proposals[1], super::propose_task(&groups[1]).unwrap());
    assert_eq!(proposals[2], super::propose_task(&groups[2]).unwrap());
}

#[test]
fn selectors_reject_unmatched_nextest_and_unknown_binary_targets() {
    let ordinary = [task_group("demo", TaskKind::Build)];
    assert!(propose_rust_groups(&ordinary, &[selector()]).is_err());
    let package = PackageRecord {
        id: "demo-id".to_owned(),
        name: "demo".to_owned(),
        version: "0.1.0".to_owned(),
        manifest: "Cargo.toml".to_owned(),
        external: false,
        in_workspace: true,
        targets: vec![TargetRecord {
            kind: "test".to_owned(),
            name: "control_states".to_owned(),
            test: true,
            doctest: false,
            required_features: Vec::new(),
        }],
        features: Vec::new(),
        has_build_script: false,
        v1_task_owner: VelnorV1TaskOwner::Project,
    };
    assert!(validate_selector_targets(std::iter::once(&package), &[selector()]).is_ok());
    let mut invalid_binary = package;
    invalid_binary.targets[0].test = false;
    assert!(validate_selector_targets(std::iter::once(&invalid_binary), &[selector()]).is_err());
}
