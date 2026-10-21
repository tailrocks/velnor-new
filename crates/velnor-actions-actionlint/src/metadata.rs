//! Workflow-lint adapter metadata routing (quality §1).
//!
//! The adapter owns the generated jactionlint config file plus lint-tool
//! metadata; sibling tool files route to their stacks and every other
//! symbol routes nowhere. Pure data: never spawns processes or reads files.

/// jactionlint config file name owned by this adapter.
pub const JACTIONLINT_CONFIG_FILE: &str = "jactionlint.yaml";

/// File symbols routed to the workflow-lint stack.
pub const OWNED_SYMBOLS: &[&str] = &["jactionlint.yaml"];

/// Tool files owned by sibling adapters; recognized but never read here.
pub const FOREIGN_TOOL_FILES: &[&str] = &[
    "mise.toml",
    ".mise.toml",
    "mise.lock",
    ".mise.lock",
    "rust-toolchain.toml",
    "Cargo.toml",
    "Cargo.lock",
];

/// Route a path to its owning stack id by file name, if any.
#[must_use]
pub fn stack_for_symbol(path: &str) -> Option<&'static str> {
    match file_name(path) {
        name if OWNED_SYMBOLS.contains(&name) => Some(crate::TOOL_ID),
        "mise.toml" | ".mise.toml" | "mise.lock" | ".mise.lock" => Some("mise"),
        "rust-toolchain.toml" | "Cargo.toml" | "Cargo.lock" => Some("rust"),
        _ => None,
    }
}

/// Whether `path` names the owned jactionlint config file.
#[must_use]
pub fn is_owned_jactionlint_file(path: &str) -> bool {
    OWNED_SYMBOLS.contains(&file_name(path))
}

/// Final path segment of a repository-relative path.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
