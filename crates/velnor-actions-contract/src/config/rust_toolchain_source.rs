//! `[stacks.rust] toolchain_source`: opt-in Mise-authoritative Rust version.

use serde::{Deserialize, Serialize};

/// Where the Rust toolchain version may be declared
/// (`[stacks.rust] toolchain_source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustToolchainSource {
    /// The project's Mise config declares the Rust version and every other
    /// declaration (`rust-toolchain.toml`/`rust-toolchain` at the root or
    /// nested, `.tool-versions`, a `.velnor/config.toml` pin) must equal it;
    /// generation fails on a mismatch and when no Mise config declares Rust.
    Mise,
}
