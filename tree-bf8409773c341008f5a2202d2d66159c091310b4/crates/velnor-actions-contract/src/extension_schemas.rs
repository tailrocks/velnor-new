//! Stack-extension schema registry (cache §1).
//!
//! Known adapter schemas; unknown schemas disable reuse and coverage.
//! Version a schema when its semantics change.

/// Extension schema emitted by the Rust adapter's identity extension.
pub const RUST_EXTENSION_SCHEMA: &str = "rust-task-identity-v1";
/// Extension schema emitted by the tofu adapter's identity extension.
pub const TOFU_EXTENSION_SCHEMA: &str = "tofu-task-identity-v1";
/// Known stack-extension schemas; unknown schemas disable reuse/coverage.
pub const KNOWN_STACK_EXTENSION_SCHEMAS: &[&str] = &[RUST_EXTENSION_SCHEMA, TOFU_EXTENSION_SCHEMA];
/// True when `schema` is a known stack-extension schema (cache §1).
#[must_use]
pub fn is_known_stack_extension_schema(schema: &str) -> bool {
    KNOWN_STACK_EXTENSION_SCHEMAS.contains(&schema)
}

#[cfg(test)]
mod tests {
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
        assert_eq!(KNOWN_STACK_EXTENSION_SCHEMAS.len(), 2);
    }
}
