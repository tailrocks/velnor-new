//! Group-identity tests (P03 matrix unit cases).
//!
//! Declared via `#[path]` from `internal_plan.rs` under `cfg(test)`.

use super::identities::*;
use super::snapshot::normalized_component_id;
use velnor_actions_contract::cachekey::{ToolchainInputs, toolchain_id};
use velnor_actions_contract::{digest_b3, validate_digest};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::{LocalEdge, TaskGroup, TaskKind, WorkspaceRecord};

/// Minimal group with kind, task ID, driver, and runner.
fn group(kind: TaskKind, task_id: &str, driver: &str, runner: &str) -> TaskGroup {
    TaskGroup {
        task_id: task_id.to_owned(),
        package_id: "demo".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: driver.to_owned(),
        test_runner: runner.to_owned(),
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    }
}

#[test]
fn lanes_follow_responsibility_never_ordinals() {
    let workspace = digest_b3(b"workspace");
    let clippy = group(
        TaskKind::Clippy,
        "stack/rust/root/clippy/default",
        "cargo",
        "cargo_test",
    );
    assert_eq!(
        lane_id_for(&clippy, &workspace),
        lane_id_for(&clippy, &workspace)
    );
    assert!(validate_digest(&lane_id_for(&clippy, &workspace)).is_ok());
    let test = group(
        TaskKind::Test,
        "stack/rust/root/test/default",
        "cargo",
        "cargo_test",
    );
    assert_ne!(
        lane_id_for(&clippy, &workspace),
        lane_id_for(&test, &workspace)
    );
    let shard_a = group(
        TaskKind::Nextest,
        "stack/rust/root/nextest/default/shard-1-of-2",
        "cargo",
        "cargo_nextest",
    );
    let shard_b = group(
        TaskKind::Nextest,
        "stack/rust/root/nextest/default/shard-2-of-2",
        "cargo",
        "cargo_nextest",
    );
    assert_ne!(
        lane_id_for(&shard_a, &workspace),
        lane_id_for(&shard_b, &workspace)
    );
    assert_eq!(writer_lane_for(&clippy.task_id), "primary");
    assert_eq!(writer_lane_for(&shard_a.task_id), "shard-1-of-2");
}

#[test]
fn toolchains_bind_sorted_specs_driver_runner() {
    let catalog = ToolCatalog::pinned();
    let cargo = group(
        TaskKind::Clippy,
        "stack/rust/root/clippy/default",
        "cargo",
        "cargo_test",
    );
    let inputs = toolchain_inputs_for(&cargo, &catalog);
    let mut sorted = inputs.tools.clone();
    sorted.sort();
    assert_eq!(inputs.tools, sorted);
    assert!(inputs.tools.iter().any(|spec| spec.starts_with("rust@")));
    let nextest = group(
        TaskKind::Nextest,
        "stack/rust/root/nextest/default",
        "cargo",
        "cargo_nextest",
    );
    assert!(toolchain_inputs_for(&nextest, &catalog).tools.len() > inputs.tools.len());
    let mbx = group(
        TaskKind::Clippy,
        "stack/rust/root/clippy/default",
        "mbx",
        "cargo_test",
    );
    assert_ne!(
        toolchain_digest_for(&cargo, &catalog).expect("digest"),
        toolchain_digest_for(&mbx, &catalog).expect("digest")
    );
    assert_ne!(
        toolchain_digest_for(&cargo, &catalog).expect("digest"),
        toolchain_digest_for(&nextest, &catalog).expect("digest")
    );
}

#[test]
fn formats_stay_single_and_graphs_relocate() {
    assert_ne!(cache_format_id_for("cargo"), cache_format_id_for("mbx"));
    assert!(validate_digest(&cache_format_id_for("cargo")).is_ok());
    assert_eq!(cache_format_id_for("bogus"), cache_format_id_for("bogus"));
    assert_ne!(cache_format_id_for("bogus"), cache_format_id_for("cargo"));
    let record = |id: &str| WorkspaceRecord {
        workspace_root: String::new(),
        members: vec![id.to_owned()],
        packages: vec![velnor_actions_rust::PackageRecord {
            id: id.to_owned(),
            name: "a".to_owned(),
            version: "0.1.0".to_owned(),
            manifest: "a/Cargo.toml".to_owned(),
            external: false,
            in_workspace: true,
            targets: Vec::new(),
            features: Vec::new(),
            has_build_script: false,
        }],
        edges: vec![LocalEdge {
            from: id.to_owned(),
            to: id.to_owned(),
            kind: velnor_actions_rust::DepKind::Normal,
            optional: false,
            target: None,
        }],
    };
    assert_eq!(
        graph_digest_for(&record("path+file:///old#a@0.1.0")),
        graph_digest_for(&record("path+file:///new#a@0.1.0"))
    );
    assert_eq!(normalized_component_id("a-id", "a/Cargo.toml"), "a-id");
}

#[test]
fn compiler_spec_versions_flip_the_digest() {
    let inputs = |tools: Vec<String>| ToolchainInputs {
        tools,
        components: vec!["clippy".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    };
    let pinned = inputs(vec!["rust@1.98.1".to_owned()]);
    let bumped = inputs(vec!["rust@1.99.0".to_owned()]);
    assert_ne!(
        toolchain_id(&pinned).expect("digest"),
        toolchain_id(&bumped).expect("digest")
    );
    // The toolchain builder feeds the same sorted specs into the same
    // digest, so a catalog pin change propagates to every task digest.
    let catalog = ToolCatalog::pinned();
    let group = group(
        TaskKind::Clippy,
        "stack/rust/root/clippy/default",
        "cargo",
        "cargo_test",
    );
    let built = toolchain_inputs_for(&group, &catalog);
    assert_eq!(
        toolchain_id(&built).expect("digest"),
        toolchain_digest_for(&group, &catalog).expect("digest")
    );
    assert!(built.tools.iter().any(|spec| spec.starts_with("rust@")));
    assert_eq!(
        built.components,
        vec!["clippy".to_owned(), "rustfmt".to_owned()]
    );
    let imaged = platform_id_for_group("ubuntu-26.04", &group);
    let older = platform_id_for_group("ubuntu-24.04", &group);
    assert_ne!(imaged, older);
    assert!(validate_digest(&imaged).is_ok());
}
