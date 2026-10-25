pub const JACTIONLINT_CONFIG_FILE: &str = "jactionlint.yaml";

pub const OWNED_SYMBOLS: &[&str] = &["jactionlint.yaml"];

pub const FOREIGN_TOOL_FILES: &[&str] = &[
    "mise.toml",
    ".mise.toml",
    "mise.lock",
    ".mise.lock",
    "rust-toolchain.toml",
    "Cargo.toml",
    "Cargo.lock",
];

#[must_use]
pub fn stack_for_symbol(path: &str) -> Option<&'static str> {
    match file_name(path) {
        name if OWNED_SYMBOLS.contains(&name) => Some(crate::TOOL_ID),
        "mise.toml" | ".mise.toml" | "mise.lock" | ".mise.lock" => Some("mise"),
        "rust-toolchain.toml" | "Cargo.toml" | "Cargo.lock" => Some("rust"),
        _ => None,
    }
}

#[must_use]
pub fn is_owned_jactionlint_file(path: &str) -> bool {
    OWNED_SYMBOLS.contains(&file_name(path))
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
