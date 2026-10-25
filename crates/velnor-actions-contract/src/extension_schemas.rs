pub const RUST_EXTENSION_SCHEMA: &str = "rust-task-identity-v1";

pub const TOFU_EXTENSION_SCHEMA: &str = "tofu-task-identity-v1";

pub const NAMED_CHECK_EXTENSION_SCHEMA: &str = "mise-named-check-identity-v1";

pub const NATIVE_HOMEBREW_CHECK_IDENTITY_SCHEMA: &str = "homebrew-native-check-identity-v2";

#[path = "named_check_extension.rs"]
pub mod named_check;

pub const KNOWN_STACK_EXTENSION_SCHEMAS: &[&str] = &[
    RUST_EXTENSION_SCHEMA,
    TOFU_EXTENSION_SCHEMA,
    NAMED_CHECK_EXTENSION_SCHEMA,
    NATIVE_HOMEBREW_CHECK_IDENTITY_SCHEMA,
];

#[must_use]
pub fn is_known_stack_extension_schema(schema: &str) -> bool {
    KNOWN_STACK_EXTENSION_SCHEMAS.contains(&schema)
}
