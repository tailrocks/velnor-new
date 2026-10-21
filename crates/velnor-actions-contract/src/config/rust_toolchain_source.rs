//! Select the Mise Rust version with `[stacks.rust] toolchain_source`.

use serde::{Deserialize, Serialize};

/// Specify where the Rust toolchain version can be declared
/// (`[stacks.rust] toolchain_source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustToolchainSource {
    /// Require a Rust version in the project's Mise config.
    /// Other declarations must use that version. These include root and nested
    /// toolchain files, `.tool-versions`, and `.velnor/config.toml` pins.
    /// Generation fails if a version differs or no Mise config declares Rust.
    Mise,
}
