//! Gate 6/7 Rust cases: identity extension, shard IDs, runner gates.

use velnor_actions_contract::cachekey::stack_extension_id;
use velnor_actions_rust::tasks::{RustTaskIdentityExtension, shard_task_id, shards_allowed};

/// Identity extension for one Nextest package task.
fn extension() -> RustTaskIdentityExtension {
    RustTaskIdentityExtension {
        package_id: "demo 0.1.0".to_owned(),
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
