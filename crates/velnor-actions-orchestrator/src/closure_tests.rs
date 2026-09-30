//! Input-closure tests (P03 matrix unit cases).
//!
//! Declared via `#[path]` from `internal_plan.rs` under `cfg(test)`.

use super::closure::*;
use super::snapshot::canonical_digest;
use velnor_actions_contract::digest_b3;
use velnor_actions_rust::{TaskGroup, TaskKind};

/// Minimal group with `declared` inputs and `reads` flag.
fn group(declared: Vec<String>, reads: bool) -> TaskGroup {
    TaskGroup {
        task_id: "stack/rust/root/clippy/default".to_owned(),
        package_id: "demo".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind: TaskKind::Clippy,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
        declared_inputs: declared,
        undeclared_reads: reads,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    }
}

/// Closure digest helper.
fn digest_of(closure: &TaskInputClosure) -> String {
    canonical_digest(closure).expect("digest")
}

#[test]
fn each_input_flips_the_closure_digest() {
    let base = ClosureBuilder::new("t")
        .value("features", "")
        .input(
            "lockfile",
            Provenance::AbsentProven {
                evidence: "none".to_owned(),
            },
        )
        .build();
    let flipped = ClosureBuilder::new("t")
        .value("features", "serde")
        .input(
            "lockfile",
            Provenance::AbsentProven {
                evidence: "none".to_owned(),
            },
        )
        .build();
    assert_ne!(digest_of(&base), digest_of(&flipped));
    let known = ClosureBuilder::new("t")
        .value("features", "")
        .input(
            "lockfile",
            Provenance::Known {
                digest: digest_b3(b"lock"),
            },
        )
        .build();
    assert_ne!(digest_of(&base), digest_of(&known));
    let unknown = ClosureBuilder::new("t")
        .value("features", "")
        .input(
            "lockfile",
            Provenance::Unknown {
                reason: "unreadable".to_owned(),
            },
        )
        .build();
    assert_ne!(digest_of(&base), digest_of(&unknown));
    assert_ne!(digest_of(&known), digest_of(&unknown));
    assert!(base.verify_complete().is_ok());
    assert!(known.verify_complete().is_ok());
    assert!(unknown.verify_complete().is_err());
    assert_eq!(unknown.unknown_inputs(), vec!["lockfile"]);
}

#[test]
fn same_path_with_changed_source_flips() {
    let before = ClosureBuilder::new("t")
        .input(
            "manifest",
            Provenance::Known {
                digest: digest_b3(b"[package]\nname = \"a\"\n"),
            },
        )
        .build();
    let after = ClosureBuilder::new("t")
        .input(
            "manifest",
            Provenance::Known {
                digest: digest_b3(b"[package]\nname = \"b\"\n"),
            },
        )
        .build();
    assert_ne!(digest_of(&before), digest_of(&after));
}

#[test]
fn unrelated_tasks_stay_isolated() {
    let left = ClosureBuilder::new("a").value("features", "").build();
    let right = ClosureBuilder::new("b").value("features", "").build();
    assert_ne!(digest_of(&left), digest_of(&right));
    let changed = ClosureBuilder::new("a").value("features", "x").build();
    assert_ne!(digest_of(&left), digest_of(&changed));
    assert_eq!(
        digest_of(&right),
        digest_of(&ClosureBuilder::new("b").value("features", "").build())
    );
}

#[test]
fn checkout_resolution_binds_content_and_absence() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("Cargo.toml"), "[package]\n").expect("manifest");
    std::fs::write(root.join("Cargo.lock"), "lock-bytes").expect("lock");
    let graph = digest_b3(b"graph");
    let toolchain = digest_b3(b"toolchain");
    let platform = digest_b3(b"platform");
    let closure = resolve_closure_at_root(
        root,
        &group(Vec::new(), false),
        None,
        &graph,
        &toolchain,
        &platform,
    );
    assert!(
        closure.verify_complete().is_ok(),
        "{:?}",
        closure.unknown_inputs()
    );
    assert!(matches!(
        closure.inputs["manifest"],
        Provenance::Known { .. }
    ));
    assert!(matches!(
        closure.inputs["nextest_config"],
        Provenance::AbsentProven { .. }
    ));
    let relocated = tempfile::tempdir().expect("tempdir");
    std::fs::write(relocated.path().join("Cargo.toml"), "[package]\n").expect("manifest");
    std::fs::write(relocated.path().join("Cargo.lock"), "lock-bytes").expect("lock");
    let again = resolve_closure_at_root(
        relocated.path(),
        &group(Vec::new(), false),
        None,
        &graph,
        &toolchain,
        &platform,
    );
    assert_eq!(digest_of(&closure), digest_of(&again));
    let missing = resolve_closure_at_root(
        root,
        &group(vec!["nope.proto".to_owned()], false),
        None,
        &graph,
        &toolchain,
        &platform,
    );
    assert!(missing.verify_complete().is_err());
    let dirty = resolve_closure_at_root(
        root,
        &group(Vec::new(), true),
        None,
        &graph,
        &toolchain,
        &platform,
    );
    assert!(dirty.verify_complete().is_err());
    assert!(dirty.unknown_inputs().contains(&"vcs"));
}
