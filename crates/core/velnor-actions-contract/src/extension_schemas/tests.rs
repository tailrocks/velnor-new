use super::*;

#[test]
fn unknown_extension_schemas_disable_identity() {
    assert!(is_known_stack_extension_schema(RUST_EXTENSION_SCHEMA));
    assert!(!is_known_stack_extension_schema("rust-task-v2"));
    assert!(!is_known_stack_extension_schema(""));
}

#[test]
fn tofu_schema_is_known() {
    assert!(is_known_stack_extension_schema(TOFU_EXTENSION_SCHEMA));
    assert!(is_known_stack_extension_schema(RUST_EXTENSION_SCHEMA));
    assert_eq!(KNOWN_STACK_EXTENSION_SCHEMAS.len(), 3);
}
