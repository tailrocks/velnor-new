//! Toolchain-home env for rendered Cargo steps (RQ-3.5, TASK-6.1).
//!
//! Every rendered Cargo step carries the owned-homes triple; values
//! arrive from the orchestrator (which reads Mise `ToolHomes`), never
//! invented here. This crate must not depend on the Mise adapter.

use std::collections::BTreeMap;

use crate::RenderError;

/// Toolchain-home env keys carried by every Cargo step.
pub const TOOLCHAIN_HOME_KEYS: [&str; 3] =
    ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"];

/// Merge the toolchain-home triple into a step env map.
#[must_use]
pub fn with_toolchain_homes(
    env: &BTreeMap<String, String>,
    rustup_home: &str,
    cargo_home: &str,
    toolchain: &str,
) -> BTreeMap<String, String> {
    let mut merged = env.clone();
    for (key, value) in [
        (TOOLCHAIN_HOME_KEYS[0], rustup_home),
        (TOOLCHAIN_HOME_KEYS[1], cargo_home),
        (TOOLCHAIN_HOME_KEYS[2], toolchain),
    ] {
        merged.insert(key.to_owned(), value.to_owned());
    }
    merged
}

/// Require the toolchain-home triple in a Cargo step env.
/// # Errors
pub fn check_toolchain_homes(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for key in TOOLCHAIN_HOME_KEYS {
        if env.get(key).is_none_or(String::is_empty) {
            return Err(RenderError::BadCommand(format!(
                "missing_toolchain_home:{key}"
            )));
        }
    }
    Ok(())
}
