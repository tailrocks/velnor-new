//! `[stacks.rust] toolchain_source`: opt-in single Rust version source.

use serde::{Deserialize, Serialize};

/// Where the Rust toolchain version may be declared
/// (`[stacks.rust] toolchain_source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustToolchainSource {
    /// The project's Mise config is the only source: generation fails on a
    /// `rust-toolchain.toml`/`rust-toolchain` file anywhere in the
    /// repository, on a Rust declaration in `.tool-versions`, on a
    /// `.velnor/config.toml` Rust pin, and when no Mise config declares Rust.
    Mise,
}
