//! M6 spike: the proposal/graph waist is opaque to adapter spellings.
//!
//! No second stack ships in Phase A, so this spike proves the waist
//! property directly instead of with a stub adapter: proposals carrying
//! distinct adapter-defined kind/driver/runner/payload spellings validate,
//! convert to nodes, and graph-validate identically. Only the closed stack
//! registry discriminates, and it fails closed at every wire gate.
use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::{ContractError, Stack, digest_b3, task_id_for_stack};
use velnor_actions_contract_config::config::VelnorConfig;
use velnor_actions_contract_planning::{
    CachePolicy, EdgeKind, IdentityInputs, ProposedTask, ResourceClass, ResourceDemand, TaskEdge,
    TaskGraph,
};

/// Identity preimage with adapter-owned driver/runner spellings.
fn identity(driver: &str, runner: &str) -> IdentityInputs {
    IdentityInputs {
        unit_id: "demo".to_owned(),
        unit_key: "root".to_owned(),
        unit_path: "Cargo.toml".to_owned(),
        project_root: ".".to_owned(),
        target: "host".to_owned(),
        features: Vec::new(),
        flags: Vec::new(),
        compile_driver: driver.to_owned(),
        test_runner: runner.to_owned(),
        environment: BTreeMap::new(),
        declared_inputs: Vec::new(),
        undeclared_reads: false,
    }
}

/// Proposal with adapter-defined kind/driver/runner/payload spellings.
fn proposal(
    stack: &str,
    kind: &str,
    driver: &str,
    runner: &str,
    argv: &[&str],
) -> Result<ProposedTask, ContractError> {
    let task_id = task_id_for_stack(stack, "root", kind, "default", None)?;
    Ok(ProposedTask {
        task_id,
        stack_id: stack.to_owned(),
        component_id: "workspace".to_owned(),
        task_kind: kind.to_owned(),
        configuration: "default".to_owned(),
        depends_on: Vec::new(),
        gated_by: Vec::new(),
        reads: Vec::new(),
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: ResourceClass::Lightweight,
            cpu_milli: None,
            memory_mb: None,
            needs_network: false,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: false,
            allow_task_reuse: false,
        },
        identity: identity(driver, runner),
        payload: argv.iter().map(OsString::from).collect(),
        display_name: "demo".to_owned(),
        uses_clock: false,
        uses_random: false,
        no_targets: false,
        runner_profile: "default".to_owned(),
    })
}

#[test]
fn waist_is_opaque_to_adapter_spellings() {
    let first = proposal(
        "rust",
        "aaa-kind",
        "driver-a",
        "runner-a",
        &["prog-a", "--flag"],
    )
    .expect("first builds");
    let second =
        proposal("rust", "zzz-kind", "driver-b", "runner-b", &["prog-b"]).expect("second builds");
    first.validate().expect("first proposes");
    second.validate().expect("second proposes");
    let mut nodes = vec![
        first.into_task_node(digest_b3(b"input-a"), digest_b3(b"lane-a")),
        second.into_task_node(digest_b3(b"input-b"), digest_b3(b"lane-b")),
    ];
    nodes.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    let graph = TaskGraph {
        nodes,
        edges: vec![TaskEdge {
            from: "stack/rust/root/aaa-kind/default".to_owned(),
            to: "stack/rust/root/zzz-kind/default".to_owned(),
            kind: EdgeKind::Data,
        }],
    };
    graph.validate().expect("graph validates");
}

#[test]
fn closed_registry_rejects_unregistered_stack_at_wire_gates() {
    assert!(Stack::require_known("cobol").is_err());
    let proposal = proposal("cobol", "validate", "cobol", "cobol", &["cobol"]).expect("builds");
    assert!(proposal.validate().is_err());
    let node = proposal.into_task_node(digest_b3(b"input"), digest_b3(b"lane"));
    assert!(node.validate().is_err());
}

#[test]
fn task_id_grammar_is_stack_parametric() {
    let rust = task_id_for_stack("rust", "root", "clippy", "default", None).expect("rust id");
    let other = task_id_for_stack("tofu", "root", "validate", "default", None).expect("grammar id");
    assert!(rust.starts_with("stack/rust/"));
    assert!(other.starts_with("stack/tofu/"));
}

/// The dispatch enum and the authoritative registry agree exactly.
#[test]
fn dispatch_enum_matches_registered_stacks() {
    let ids: Vec<&str> = Stack::all().iter().map(|stack| stack.id()).collect();
    assert_eq!(ids, VelnorConfig::REGISTERED_STACKS);
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "registry runs ascending");
    assert_eq!(Stack::require_known("rust"), Ok(Stack::Rust));
    assert_eq!(Stack::require_known("tofu"), Ok(Stack::Tofu));
}
