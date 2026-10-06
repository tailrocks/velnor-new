use super::super::propose_task;
use super::group;
use crate::tasks::{TaskKind, cargo_payload_with_profile};
use velnor_actions_contract_planning::ResourceClass;
use velnor_actions_rust_core::profile::{CompileDriver, TestRunner};

/// Conversion copies ids/edges and precomputes adapter facts.
#[test]
fn conversion_copies_and_precomputes() {
    let group = group();
    let task = propose_task(&group).expect("valid group proposes");
    assert_eq!(task.task_id, group.task_id);
    assert_eq!(task.stack_id, crate::STACK_ID);
    assert_eq!(task.component_id, "demo@0.1.0");
    assert_eq!(task.task_kind, "clippy");
    assert_eq!(task.reads, vec!["Cargo.toml".to_owned()]);
    assert_eq!(task.identity.unit_id, group.package_id);
    assert_eq!(task.identity.unit_key, "root");
    assert_eq!(task.identity.unit_path, "Cargo.toml");
    assert_eq!(task.identity.project_root, ".");
    assert_eq!(task.identity.compile_driver, "cargo");
    assert!(task.validate().is_ok());
}

/// The precomputed payload matches the direct payload bytes.
#[test]
fn payload_matches_direct_bytes() {
    let group = group();
    let task = propose_task(&group).expect("valid group proposes");
    assert_eq!(
        task.payload,
        cargo_payload_with_profile(&group).expect("direct payload")
    );
}

/// Payloads match for every kind with kind-specific inputs loaded.
#[test]
fn payload_matches_all_kinds() {
    let kinds = [
        TaskKind::Fmt,
        TaskKind::Clippy,
        TaskKind::Build,
        TaskKind::Test,
        TaskKind::Nextest,
        TaskKind::Doctest,
        TaskKind::Doc,
    ];
    for kind in kinds {
        let mut group = group();
        group.kind = kind;
        group.target_flags = vec!["--lib".to_owned()];
        group.no_test_targets = false;
        let task = propose_task(&group).expect("valid group proposes");
        assert_eq!(
            task.payload,
            cargo_payload_with_profile(&group).expect("direct payload"),
            "payload drift for kind {}",
            kind.as_str()
        );
        assert!(task.validate().is_ok(), "kind {} validates", kind.as_str());
        assert_eq!(
            task.identity.environment.is_empty(),
            kind != TaskKind::Doc,
            "env for kind {}",
            kind.as_str()
        );
    }
}

/// Conversion copies every field, including edge-case spellings.
#[test]
fn conversion_copies_every_field() {
    let mut group = group();
    group.manifest_key = "crates/a".to_owned();
    group.kind = TaskKind::Nextest;
    group.features = vec!["a".to_owned(), "b".to_owned()];
    group.target = "x86_64-unknown-linux-gnu".to_owned();
    group.gated_by = vec!["stack/rust/root/clippy/default".to_owned()];
    group.depends_on = vec!["stack/rust/root/build/default".to_owned()];
    group.target_flags = vec!["--tests".to_owned()];
    group.no_test_targets = true;
    group.compile_driver = CompileDriver::Mbx;
    group.test_runner = TestRunner::CargoNextest;
    group.declared_inputs = vec!["proto/a.proto".to_owned()];
    group.undeclared_reads = true;
    group.uses_network = true;
    group.uses_clock = true;
    group.uses_random = true;
    let task = propose_task(&group).expect("valid group proposes");
    assert_eq!(task.task_kind, "nextest");
    assert_eq!(task.configuration, group.configuration);
    assert_eq!(task.depends_on, group.depends_on);
    assert_eq!(task.gated_by, group.gated_by);
    assert_eq!(task.reads, vec!["crates/a/Cargo.toml".to_owned()]);
    assert!(task.writes.is_empty() && task.outputs.is_empty());
    assert_eq!(task.resource.class, ResourceClass::Test);
    assert!(task.resource.needs_network);
    assert!(task.cache_policy.allow_compilation_reuse);
    assert!(!task.cache_policy.allow_task_reuse);
    let identity = &task.identity;
    assert_eq!(identity.unit_id, group.package_id);
    assert_eq!(identity.unit_key, "crates/a");
    assert_eq!(identity.unit_path, "crates/a/Cargo.toml");
    assert_eq!(identity.project_root, "crates/a");
    assert_eq!(identity.target, group.target);
    assert_eq!(identity.features, group.features);
    assert_eq!(identity.flags, group.target_flags);
    assert_eq!(identity.compile_driver, "mbx");
    assert_eq!(identity.test_runner, "cargo_nextest");
    assert_eq!(identity.declared_inputs, group.declared_inputs);
    assert!(identity.undeclared_reads);
    assert!(identity.environment.is_empty());
    assert_eq!(task.display_name, group.package_name);
    assert!(task.uses_clock && task.uses_random && task.no_targets);
    assert_eq!(task.runner_profile, "default");
    assert_eq!(task.component_id, "demo@0.1.0");
    assert!(task.validate().is_ok());
}

/// Leading-dash values fail the proposal with the payload error.
#[test]
fn proposal_rejects_leading_dash_target() {
    let mut group = group();
    group.target = "-bad".to_owned();
    let err = propose_task(&group).expect_err("leading dash must fail");
    assert!(err.to_string().contains("leading_dash_target"), "{err}");
}
