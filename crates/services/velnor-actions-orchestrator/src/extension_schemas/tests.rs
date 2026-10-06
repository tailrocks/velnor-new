use super::*;

#[test]
fn segments_parse_nested_keys_shards_and_internal() {
    assert_eq!(
        task_stack_segment("stack/rust/a/b/clippy/default"),
        Some("rust")
    );
    assert_eq!(
        task_kind_segment("stack/rust/a/b/clippy/default"),
        Some("clippy")
    );
    assert_eq!(
        task_kind_segment("stack/rust/root/nextest/default/shard-2-of-4"),
        Some("nextest")
    );
    assert_eq!(task_kind_segment("internal/policy/default"), Some("policy"));
    assert_eq!(task_kind_segment("stack/rust/root"), None);
    assert_eq!(task_stack_segment("internal/policy/default"), None);
    assert_eq!(task_stack_segment("bogus"), None);
}

#[test]
fn key_segment_returns_nested_manifest_keys() {
    assert_eq!(
        task_key_segment("stack/tofu/root/validate/default"),
        Some("root".to_owned())
    );
    assert_eq!(
        task_key_segment("stack/tofu/stacks/vpc/fmt/default"),
        Some("stacks/vpc".to_owned())
    );
    assert_eq!(
        task_key_segment("stack/tofu/stacks/a/init/default/shard-1-of-2"),
        Some("stacks/a".to_owned())
    );
    assert_eq!(
        task_key_segment("stack/rust/a/b/clippy/default"),
        Some("a/b".to_owned())
    );
    assert_eq!(task_key_segment("internal/policy/default"), None);
    assert_eq!(task_key_segment("stack/rust/root"), None);
    assert_eq!(task_key_segment("stack//root/clippy/default"), None);
    assert_eq!(task_key_segment("bogus"), None);
}

#[test]
fn known_rust_covers_but_unknown_stacks_do_not() {
    assert_eq!(
        extension_schema_for_stack("rust"),
        Some(RUST_EXTENSION_SCHEMA)
    );
    assert_eq!(
        extension_schema_for_stack("mise"),
        Some(velnor_actions_contract::NAMED_CHECK_EXTENSION_SCHEMA)
    );
    assert!(coverage_schema_known("stack/rust/root/clippy/default"));
    assert!(!coverage_schema_known("stack/unknown/root/test/default"));
    assert!(coverage_schema_known("internal/policy/default"));
    assert!(!coverage_schema_known("bogus"));
    assert!(reuse_eligible_for_schema(RUST_EXTENSION_SCHEMA));
    assert!(!reuse_eligible_for_schema("rust-task-v2"));
}

#[test]
fn tofu_schema_covers_tofu_tasks() {
    assert_eq!(
        extension_schema_for_stack("tofu"),
        Some(TOFU_EXTENSION_SCHEMA)
    );
    assert!(coverage_schema_known("stack/tofu/root/validate/default"));
    assert!(coverage_schema_known(
        "stack/tofu/stacks/a/init/default/shard-1-of-2"
    ));
    assert!(reuse_eligible_for_schema(TOFU_EXTENSION_SCHEMA));
}
