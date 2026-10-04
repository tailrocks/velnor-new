//! Fail closed for restored executable trees without authenticated inventories.
//!
//! A qualified manager hash does not authenticate its installed tool payloads.
//! The current fallback retains only that manager and installs a fresh tree.
//! Signed producer receipt verification must precede any future warm admission.

#[path = "catalog_native_root_clear.rs"]
mod root_clear;

/// Remove canonical selected root leaves before receipt materialization.
#[must_use]
pub fn root_clear_script(domain: velnor_actions_contract::ToolCacheDomain) -> String {
    root_clear::source(domain)
}

/// Fixed preinstallation source. Arguments bind the owner's exact root and
/// already-qualified Mise binary digest; repository data never selects code.
#[must_use]
pub fn cold_prepare_script() -> String {
    let code = format!(
        "exec({BOUNDS:?}); exec({SOURCE:?}); import sys; cold_prepare(sys.argv[1], sys.argv[2], sys.argv[3])"
    );
    let code = code.replace('\'', "'\\''");
    format!("/usr/bin/python3 -I -S -c '{code}' \"$root\" \"$sha\" \"${{RUNNER_TEMP:?}}\"")
}

const SOURCE: &str = include_str!("catalog_native_health.py");

const BOUNDS: &str = include_str!("catalog_executable_bounds.py");
