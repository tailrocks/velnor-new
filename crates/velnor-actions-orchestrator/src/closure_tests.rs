//! Input-closure tests (P03 matrix unit cases).
//!
//! Declared via `#[path]` from `internal_plan.rs` under `cfg(test)`.

use super::closure::*;
use super::closure_slots::{lock_digest_at_root, nextest_digest_at_root};
use super::snapshot::canonical_digest;
use velnor_actions_contract::{ProposedTask, Provenance, TaskInputClosure, digest_b3};
use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};

/// Minimal proposal with `declared` inputs and `reads` flag.
fn group(declared: Vec<String>, reads: bool) -> ProposedTask {
    let group = TaskGroup {
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
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        declared_inputs: declared,
        undeclared_reads: reads,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        nextest_profile: NextestProfile::Default,
    };
    let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

/// Closure digest helper.
fn digest_of(closure: &TaskInputClosure) -> String {
    canonical_digest(closure).expect("digest")
}

#[test]
fn each_input_flips_the_closure_digest() {
    let base = ClosureBuilder::new()
        .value("features", "")
        .input(
            "lockfile",
            Provenance::AbsentProven {
                evidence: "none".to_owned(),
            },
        )
        .build("t");
    let flipped = ClosureBuilder::new()
        .value("features", "serde")
        .input(
            "lockfile",
            Provenance::AbsentProven {
                evidence: "none".to_owned(),
            },
        )
        .build("t");
    assert_ne!(digest_of(&base), digest_of(&flipped));
    let known = ClosureBuilder::new()
        .value("features", "")
        .input(
            "lockfile",
            Provenance::Known {
                digest: digest_b3(b"lock"),
            },
        )
        .build("t");
    assert_ne!(digest_of(&base), digest_of(&known));
    let unknown = ClosureBuilder::new()
        .value("features", "")
        .input(
            "lockfile",
            Provenance::Unknown {
                reason: "unreadable".to_owned(),
            },
        )
        .build("t");
    assert_ne!(digest_of(&base), digest_of(&unknown));
    assert_ne!(digest_of(&known), digest_of(&unknown));
    assert!(base.verify_complete().is_ok());
    assert!(known.verify_complete().is_ok());
    assert!(unknown.verify_complete().is_err());
    assert_eq!(unknown.unknown_inputs(), vec!["lockfile"]);
}

#[test]
fn same_path_with_changed_source_flips() {
    let before = ClosureBuilder::new()
        .input(
            "manifest",
            Provenance::Known {
                digest: digest_b3(b"[package]\nname = \"a\"\n"),
            },
        )
        .build("t");
    let after = ClosureBuilder::new()
        .input(
            "manifest",
            Provenance::Known {
                digest: digest_b3(b"[package]\nname = \"b\"\n"),
            },
        )
        .build("t");
    assert_ne!(digest_of(&before), digest_of(&after));
}

#[test]
fn unrelated_tasks_stay_isolated() {
    let left = ClosureBuilder::new().value("features", "").build("a");
    let right = ClosureBuilder::new().value("features", "").build("b");
    assert_ne!(digest_of(&left), digest_of(&right));
    let changed = ClosureBuilder::new().value("features", "x").build("a");
    assert_ne!(digest_of(&left), digest_of(&changed));
    assert_eq!(
        digest_of(&right),
        digest_of(&ClosureBuilder::new().value("features", "").build("b"))
    );
}

#[test]
fn checkout_resolution_binds_content_and_absence() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("Cargo.toml"), "[package]\n").expect("manifest");
    std::fs::write(root.join("Cargo.lock"), "lock-bytes").expect("lock");
    std::fs::create_dir(root.join("src")).expect("src");
    std::fs::write(root.join("src/lib.rs"), "pub fn f() {}\n").expect("source");
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
    )
    .expect("closure");
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
        closure.inputs["source_tree"],
        Provenance::Known { .. }
    ));
    assert!(matches!(
        closure.inputs["nextest_config"],
        Provenance::AbsentProven { .. }
    ));
    let relocated = tempfile::tempdir().expect("tempdir");
    std::fs::write(relocated.path().join("Cargo.toml"), "[package]\n").expect("manifest");
    std::fs::write(relocated.path().join("Cargo.lock"), "lock-bytes").expect("lock");
    std::fs::create_dir(relocated.path().join("src")).expect("src");
    std::fs::write(relocated.path().join("src/lib.rs"), "pub fn f() {}\n").expect("source");
    let again = resolve_closure_at_root(
        relocated.path(),
        &group(Vec::new(), false),
        None,
        &graph,
        &toolchain,
        &platform,
    )
    .expect("closure");
    assert_eq!(digest_of(&closure), digest_of(&again));
    let missing = resolve_closure_at_root(
        root,
        &group(vec!["nope.proto".to_owned()], false),
        None,
        &graph,
        &toolchain,
        &platform,
    )
    .expect("closure");
    assert!(missing.verify_complete().is_err());
    let dirty = resolve_closure_at_root(
        root,
        &group(Vec::new(), true),
        None,
        &graph,
        &toolchain,
        &platform,
    )
    .expect("closure");
    assert!(dirty.verify_complete().is_err());
    assert!(dirty.unknown_inputs().contains(&"vcs"));
}

#[test]
fn source_edits_flip_and_classes_exclude_explicitly() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("Cargo.toml"), "[package]\n").expect("manifest");
    std::fs::create_dir(root.join("src")).expect("src");
    std::fs::write(root.join("src/lib.rs"), "pub fn f() {}\n").expect("source");
    let graph = digest_b3(b"graph");
    let toolchain = digest_b3(b"toolchain");
    let platform = digest_b3(b"platform");
    let resolve = |root: &std::path::Path| {
        resolve_closure_at_root(
            root,
            &group(Vec::new(), false),
            None,
            &graph,
            &toolchain,
            &platform,
        )
        .expect("closure")
    };
    let before = digest_of(&resolve(root));
    std::fs::write(root.join("src/lib.rs"), "pub fn f() {}\npub fn g() {}\n").expect("edit");
    assert_ne!(before, digest_of(&resolve(root)));
    let clippy = resolve(root);
    for (name, marker) in [
        ("docs", "kind_does_not_render_docs"),
        ("fixtures", "kind_does_not_execute_tests"),
    ] {
        match &clippy.inputs[name] {
            Provenance::AbsentProven { evidence } => {
                assert!(evidence.contains(marker), "{evidence}");
            }
            other => panic!("{name} must exclude explicitly: {other:?}"),
        }
    }
    assert!(clippy.verify_complete().is_ok());
}

#[test]
fn digest_slots_preserve_absence_distinctly() {
    use velnor_actions_rust::tasks::DigestSlot;
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("Cargo.toml"), "[package]\n").expect("manifest");
    assert!(matches!(
        lock_digest_at_root(root, "Cargo.toml"),
        DigestSlot::AbsentProven(_)
    ));
    assert!(matches!(
        nextest_digest_at_root(root, None),
        DigestSlot::AbsentProven(_)
    ));
    std::fs::write(root.join("Cargo.lock"), "lock-bytes").expect("lock");
    assert!(matches!(
        lock_digest_at_root(root, "Cargo.toml"),
        DigestSlot::Known(_)
    ));
    std::fs::create_dir_all(root.join(".config")).expect("config dir");
    std::fs::write(root.join(".config/nextest.toml"), "[profile.ci]\n").expect("nextest");
    assert!(matches!(
        nextest_digest_at_root(root, None),
        DigestSlot::Known(_)
    ));
}
