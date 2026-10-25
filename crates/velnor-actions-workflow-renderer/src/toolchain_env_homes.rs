use std::collections::BTreeMap;

use crate::RenderError;

use super::{reject_denied_step_keys, reject_denied_tf_keys, reject_privileged_task_keys};

/// Toolchain-home env keys carried by every Cargo step.
pub const TOOLCHAIN_HOME_KEYS: [&str; 5] = [
    "MISE_RUSTUP_HOME",
    "MISE_CARGO_HOME",
    "RUSTUP_TOOLCHAIN",
    "RUSTUP_HOME",
    "CARGO_HOME",
];

/// Merge matching Mise and direct toolchain homes into a step env map.
/// A non-empty `toolchain` also adds `RUSTUP_TOOLCHAIN`.
#[must_use]
pub fn with_toolchain_homes(
    env: &BTreeMap<String, String>,
    rustup_home: &str,
    cargo_home: &str,
    toolchain: &str,
) -> BTreeMap<String, String> {
    let mut merged = env.clone();
    merged.insert(TOOLCHAIN_HOME_KEYS[0].to_owned(), rustup_home.to_owned());
    merged.insert(TOOLCHAIN_HOME_KEYS[1].to_owned(), cargo_home.to_owned());
    merged.insert(TOOLCHAIN_HOME_KEYS[3].to_owned(), rustup_home.to_owned());
    merged.insert(TOOLCHAIN_HOME_KEYS[4].to_owned(), cargo_home.to_owned());
    if !toolchain.is_empty() {
        merged.insert(TOOLCHAIN_HOME_KEYS[2].to_owned(), toolchain.to_owned());
    }
    merged
}

/// Require matching non-empty Mise and direct homes and an optional non-empty toolchain.
/// # Errors
pub fn check_toolchain_homes(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for key in [
        TOOLCHAIN_HOME_KEYS[0],
        TOOLCHAIN_HOME_KEYS[1],
        TOOLCHAIN_HOME_KEYS[3],
        TOOLCHAIN_HOME_KEYS[4],
    ] {
        if env.get(key).is_none_or(String::is_empty) {
            return Err(RenderError::BadCommand(format!(
                "missing_toolchain_home:{key}"
            )));
        }
    }
    for (mise_key, direct_key) in [
        (TOOLCHAIN_HOME_KEYS[0], TOOLCHAIN_HOME_KEYS[3]),
        (TOOLCHAIN_HOME_KEYS[1], TOOLCHAIN_HOME_KEYS[4]),
    ] {
        if env.get(mise_key) != env.get(direct_key) {
            return Err(RenderError::BadCommand(format!(
                "conflicting_toolchain_home:{mise_key}:{direct_key}"
            )));
        }
    }
    if env
        .get(TOOLCHAIN_HOME_KEYS[2])
        .is_some_and(String::is_empty)
    {
        return Err(RenderError::BadCommand(format!(
            "missing_toolchain_home:{}",
            TOOLCHAIN_HOME_KEYS[2]
        )));
    }
    Ok(())
}

/// Merge the triple over untrusted project-task declarations, refusing all privileged keys.
///
/// Same contract as [`checked_task_env`], plus isolation-key and
/// TF-family denial: a hostile project task cannot re-enable
/// hooks/config, steer `IaC` tool behavior, or smuggle credentials
/// through its declared env.
/// # Errors
pub fn checked_project_task_env(
    base: &BTreeMap<String, String>,
    rustup_home: &str,
    cargo_home: &str,
    toolchain: &str,
) -> Result<BTreeMap<String, String>, RenderError> {
    reject_privileged_task_keys(base)?;
    reject_denied_tf_keys(base)?;
    checked_task_env(base, rustup_home, cargo_home, toolchain)
}

/// Merge the triple over a validated base, refusing blanks and credentials.
///
/// The base carries the caller's policy pairs plus step extras; blank
/// triple inputs and denied credential keys anywhere in the merged map
/// fail closed before any step renders.
/// # Errors
pub fn checked_task_env(
    base: &BTreeMap<String, String>,
    rustup_home: &str,
    cargo_home: &str,
    toolchain: &str,
) -> Result<BTreeMap<String, String>, RenderError> {
    for (key, value) in [
        (TOOLCHAIN_HOME_KEYS[0], rustup_home),
        (TOOLCHAIN_HOME_KEYS[1], cargo_home),
    ] {
        if value.is_empty() {
            return Err(RenderError::BadCommand(format!(
                "missing_toolchain_home:{key}"
            )));
        }
    }
    let merged = with_toolchain_homes(base, rustup_home, cargo_home, toolchain);
    check_toolchain_homes(&merged)?;
    reject_denied_step_keys(&merged)?;
    Ok(merged)
}
