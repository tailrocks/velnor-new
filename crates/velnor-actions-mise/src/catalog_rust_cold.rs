//! Cold admission for Rust executable payloads without authenticated receipts.
//!
//! Writable inventory markers cannot authenticate a restored compiler. Reset
//! Full Mise execution state, every Rustup entry and Cargo's executable payload
//! before pinned bootstrap; retain only an independently qualified Mise manager.

/// Clear the canonical Full payload before receipt admission and bootstrap.
#[must_use]
pub fn clear_script() -> String {
    crate::catalog::native_health::root_clear_script(velnor_actions_contract::ToolCacheDomain::Full)
}

/// Fixed source for the isolated owner; no restored manager executes here.
#[must_use]
pub fn prepare_script(mise: &super::qualification::QualifiedDistribution) -> String {
    let code = format!(
        "exec({BOUNDS:?}); exec({COMMON:?}); exec({SOURCE:?}); import os; rust_cold_prepare(os.environ[\"RUNNER_TEMP\"], os.environ[\"CARGO_HOME\"], os.environ[\"RUSTUP_HOME\"], {expected:?})",
        expected = mise.binary_sha256(),
    );
    let code = code.replace('\'', "'\\''");
    format!("/usr/bin/python3 -I -S -c '{code}'")
}

const COMMON: &str = include_str!("catalog_native_health.py");
const SOURCE: &str = include_str!("catalog_rust_cold.py");

const BOUNDS: &str = include_str!("catalog_executable_bounds.py");
