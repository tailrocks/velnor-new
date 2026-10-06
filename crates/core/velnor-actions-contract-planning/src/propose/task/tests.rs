use super::*;
use crate::graph::ResourceClass;

/// Minimal valid proposal for validator tests.
fn proposal() -> ProposedTask {
    ProposedTask {
        task_id: "stack/rust/root/clippy/default".to_owned(),
        stack_id: "rust".to_owned(),
        component_id: "demo@0.1.0".to_owned(),
        task_kind: "clippy".to_owned(),
        configuration: "default".to_owned(),
        depends_on: Vec::new(),
        gated_by: Vec::new(),
        reads: vec!["Cargo.toml".to_owned()],
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: ResourceClass::Compiler,
            cpu_milli: None,
            memory_mb: None,
            needs_network: false,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: true,
            allow_task_reuse: true,
        },
        identity: IdentityInputs {
            unit_id: "demo".to_owned(),
            unit_key: "root".to_owned(),
            unit_path: "Cargo.toml".to_owned(),
            project_root: ".".to_owned(),
            target: "host".to_owned(),
            features: Vec::new(),
            flags: Vec::new(),
            compile_driver: "cargo".to_owned(),
            test_runner: "cargo_test".to_owned(),
            environment: std::collections::BTreeMap::new(),
            declared_inputs: Vec::new(),
            undeclared_reads: false,
        },
        payload: vec![OsString::from("clippy")],
        display_name: "demo".to_owned(),
        uses_clock: false,
        uses_random: false,
        no_targets: false,
        runner_profile: "default".to_owned(),
    }
}

/// Validation accepts the minimal proposal and rejects drift.
#[test]
fn validation_pins_proposal_shape() {
    assert!(proposal().validate().is_ok());
    let unregistered = ProposedTask {
        stack_id: "cobol".to_owned(),
        ..proposal()
    };
    assert!(unregistered.validate().is_err());
    let unordered = ProposedTask {
        depends_on: vec!["b".to_owned(), "a".to_owned()],
        ..proposal()
    };
    assert!(unordered.validate().is_err());
    let empty_payload = ProposedTask {
        payload: Vec::new(),
        ..proposal()
    };
    assert!(empty_payload.validate().is_err());
}

/// Node completion preserves every proposed field verbatim.
#[test]
fn node_completion_copies_fields() {
    let task = proposal();
    let node = task
        .clone()
        .into_task_node("input".to_owned(), "lane".to_owned());
    assert_eq!(node.task_id, task.task_id);
    assert_eq!(node.stack_id, task.stack_id);
    assert_eq!(node.component_id, task.component_id);
    assert_eq!(node.task_kind, task.task_kind);
    assert_eq!(node.configuration, task.configuration);
    assert_eq!(node.input_digest, "input");
    assert_eq!(node.lane_id, "lane");
    assert_eq!(node.reads, task.reads);
}
