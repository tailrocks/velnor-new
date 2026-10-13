//! Gate 6/7 Rust cases: identity extension, shard IDs, runner gates.

use velnor_actions_contract::cachekey::stack_extension_id;
use velnor_actions_rust::tasks::{
    ExtensionInputs, RustTaskIdentityExtension, TaskKind, parse_rerun_changed,
    require_nextest_for_shards, shard_task_id, shards_allowed,
};

/// Identity extension for one Nextest package task.
fn extension() -> RustTaskIdentityExtension {
    RustTaskIdentityExtension {
        package_id: "demo 0.1.0".to_owned(),
        workspace_id: "workspace".to_owned(),
        profile: "default".to_owned(),
        manifest: "Cargo.toml".to_owned(),
        graph_digest: velnor_actions_contract::digest_b3(b"graph"),
        targets: vec!["lib".to_owned()],
        features: vec!["default".to_owned()],
        target: "host".to_owned(),
        driver: "cargo".to_owned(),
        config_digest: velnor_actions_contract::digest_b3(b"config"),
        nextest_digest: Some(velnor_actions_contract::digest_b3(b"nextest")),
        kind: "nextest".to_owned(),
        undeclared_reads: false,
        lock_digest: None,
        archive: None,
        rerun_inputs: Vec::new(),
        declared_inputs: Vec::new(),
    }
}

#[test]
fn extension_serializes_and_gates_reuse() {
    let ext = extension();
    assert!(ext.reuse_eligible().is_ok());
    let wrapped = ext.to_stack_extension();
    assert_eq!(wrapped.schema, "rust-task-identity-v1");
    assert!(stack_extension_id(&wrapped).is_ok());
    let dirty = RustTaskIdentityExtension {
        undeclared_reads: true,
        ..ext
    };
    assert!(
        dirty.reuse_eligible().is_err(),
        "undeclared reads disable reuse"
    );
}

#[test]
fn shard_ids_derive_only_from_clean_bases() {
    let base = "stack/rust/root/nextest/default";
    assert_eq!(
        shard_task_id(base, 1, 2).expect("shard"),
        "stack/rust/root/nextest/default/shard-1-of-2"
    );
    assert_eq!(
        shard_task_id(base, 2, 2).expect("shard"),
        "stack/rust/root/nextest/default/shard-2-of-2"
    );
    assert!(shard_task_id(base, 0, 2).is_err(), "index zero rejected");
    assert!(
        shard_task_id(base, 3, 2).is_err(),
        "index above count rejected"
    );
    assert!(shard_task_id(base, 1, 0).is_err(), "zero count rejected");
    let sharded = "stack/rust/root/nextest/default/shard-1-of-2";
    assert!(
        shard_task_id(sharded, 1, 2).is_err(),
        "double sharding rejected"
    );
    assert!(shard_task_id("not-a-task-id", 1, 2).is_err());
    assert!(shard_task_id("stack/rust", 1, 2).is_err());
}

#[test]
fn only_nextest_shards() {
    assert!(shards_allowed("cargo_nextest"));
    assert!(!shards_allowed("cargo_test"));
    assert!(!shards_allowed(""));
}

/// Constructor inputs for one Nextest test task.
fn constructor_inputs<'a>(
    targets: &'a [String],
    features: &'a [String],
    declared: &'a [String],
) -> ExtensionInputs<'a> {
    ExtensionInputs {
        package_id: "demo 0.1.0",
        workspace_id: "workspace",
        profile: "default",
        manifest: "Cargo.toml",
        graph_digest: "graph",
        targets,
        features,
        target: "host",
        driver: "cargo",
        runner: "cargo_nextest",
        config_digest: "config",
        lock_digest: Some("lock"),
        nextest_digest: Some("nextest"),
        kind: TaskKind::Nextest,
        archive_source: None,
        rerun_inputs: Some(&[]),
        has_build_script: false,
        declared_inputs: declared,
    }
}

#[test]
fn extension_constructor_records_identity_before_selection() {
    let targets = vec!["lib".to_owned(), "bin".to_owned()];
    let features = vec!["zeta".to_owned(), "alpha".to_owned()];
    let declared = vec!["tests/data/corpus.md".to_owned()];
    let inputs = constructor_inputs(&targets, &features, &declared);
    let ext = RustTaskIdentityExtension::for_task(&inputs);
    assert_eq!(ext.package_id, "demo 0.1.0");
    assert_eq!(ext.manifest, "Cargo.toml");
    assert_eq!(ext.targets, vec!["bin".to_owned(), "lib".to_owned()]);
    assert_eq!(ext.features, vec!["alpha".to_owned(), "zeta".to_owned()]);
    assert_eq!(ext.kind, "nextest");
    assert_eq!(ext.driver, "cargo+cargo_nextest");
    assert_eq!(ext.lock_digest.as_deref(), Some("lock"));
    assert_eq!(ext.declared_inputs, declared);
    assert!(!ext.undeclared_reads);
    assert!(ext.reuse_eligible().is_ok());
    assert_eq!(ext.to_stack_extension().schema, "rust-task-identity-v1");
}

#[test]
fn rerun_inputs_recorded_and_unknown_disables_reuse() {
    let output = "cargo::rerun-if-changed=proto/a.proto\nnoise\ncargo:rerun-if-changed=proto/b.proto\ncargo::rerun-if-changed=\n";
    assert_eq!(
        parse_rerun_changed(output),
        vec!["proto/a.proto".to_owned(), "proto/b.proto".to_owned()]
    );
    assert!(parse_rerun_changed("cargo:rustc-link-lib=native\n").is_empty());
    let targets = vec!["lib".to_owned()];
    let features = vec!["default".to_owned()];
    let declared: Vec<String> = Vec::new();
    let mut inputs = constructor_inputs(&targets, &features, &declared);
    inputs.has_build_script = true;
    inputs.rerun_inputs = None;
    let unknown = RustTaskIdentityExtension::for_task(&inputs);
    assert!(unknown.undeclared_reads);
    assert!(unknown.reuse_eligible().is_err());
    let known = vec!["proto/a.proto".to_owned()];
    inputs.rerun_inputs = Some(&known);
    let declared_ext = RustTaskIdentityExtension::for_task(&inputs);
    assert!(!declared_ext.undeclared_reads);
    assert_eq!(declared_ext.rerun_inputs, known);
    assert!(declared_ext.reuse_eligible().is_ok());
}

#[test]
fn cargo_test_profiles_never_carry_archives() {
    let targets = vec!["lib".to_owned()];
    let features = vec!["default".to_owned()];
    let declared: Vec<String> = Vec::new();
    let base = constructor_inputs(&targets, &features, &declared);
    let mut build = base.clone();
    build.kind = TaskKind::Build;
    build.runner = "cargo_test";
    build.archive_source = Some("stack/rust/root/build/default");
    assert!(
        RustTaskIdentityExtension::for_task(&build)
            .archive
            .is_none(),
        "cargo-test must never archive"
    );
    let mut test = base.clone();
    test.kind = TaskKind::Test;
    test.runner = "cargo_test";
    test.archive_source = Some("stack/rust/root/build/default");
    assert!(
        RustTaskIdentityExtension::for_task(&test).archive.is_none(),
        "cargo-test obligation carries no archive"
    );
    let mut nextest_build = base;
    nextest_build.kind = TaskKind::Build;
    nextest_build.archive_source = Some("stack/rust/root/build/default");
    assert_eq!(
        RustTaskIdentityExtension::for_task(&nextest_build).archive,
        Some("stack/rust/root/build/default".to_owned()),
        "nextest archive names its source build"
    );
}

#[test]
fn shards_require_nextest() {
    assert!(require_nextest_for_shards("cargo_nextest", 4).is_ok());
    assert!(require_nextest_for_shards("cargo_test", 1).is_ok());
    assert!(require_nextest_for_shards("cargo_test", 2).is_err());
    assert!(require_nextest_for_shards("", 2).is_err());
}

/// Serialize one derived extension for identity comparisons.
fn json_of(ext: &RustTaskIdentityExtension) -> Option<serde_json::Value> {
    serde_json::to_value(ext).ok()
}

#[test]
fn identity_differs_across_runner_features_target() {
    let targets = vec!["lib".to_owned()];
    let features = vec!["default".to_owned()];
    let declared: Vec<String> = Vec::new();
    let base = constructor_inputs(&targets, &features, &declared);
    let base_json = json_of(&RustTaskIdentityExtension::for_task(&base));
    assert!(base_json.is_some(), "extension must serialize");
    let mut other = base.clone();
    other.runner = "cargo_test";
    let runner_json = json_of(&RustTaskIdentityExtension::for_task(&other));
    assert_ne!(base_json, runner_json);
    let alt_features = vec!["extra".to_owned()];
    let mut alt = base.clone();
    alt.features = &alt_features;
    let alt_json = json_of(&RustTaskIdentityExtension::for_task(&alt));
    assert_ne!(base_json, alt_json);
    let mut targeted = base;
    targeted.target = "x86_64-unknown-linux-gnu";
    let targeted_json = json_of(&RustTaskIdentityExtension::for_task(&targeted));
    assert_ne!(base_json, targeted_json);
}
