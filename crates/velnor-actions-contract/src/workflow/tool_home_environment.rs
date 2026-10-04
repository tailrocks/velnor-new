//! Canonical Full payload homes; bindings describe locations, not tool authority.

use std::collections::BTreeMap;

use super::ToolCacheDomain;

/// Cargo proxy/metadata and separately transported source namespace.
pub const CARGO_HOME: &str = "${{ runner.temp }}/velnor/cargo";
/// Rustup settings, installed toolchains and manager-owned state.
pub const RUSTUP_HOME: &str = "${{ runner.temp }}/velnor/rustup";

/// Bind aliases to the same stored roots without selecting a compiler.
#[must_use]
pub fn bind(rustup: &str, cargo: &str) -> [(&'static str, String); 4] {
    [
        ("CARGO_HOME", cargo.to_owned()),
        ("MISE_CARGO_HOME", cargo.to_owned()),
        ("MISE_RUSTUP_HOME", rustup.to_owned()),
        ("RUSTUP_HOME", rustup.to_owned()),
    ]
}

/// Only Full may contain Rust payloads; other domains are isolated owners.
#[must_use]
pub(super) fn for_domain(domain: ToolCacheDomain) -> BTreeMap<String, String> {
    if domain != ToolCacheDomain::Full {
        return BTreeMap::new();
    }
    bind(RUSTUP_HOME, CARGO_HOME)
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}
