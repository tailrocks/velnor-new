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

/// Credential keys forbidden in any rendered step env.
///
/// The Mise GitHub token, its `GITHUB_TOKEN`/`GH_TOKEN` aliases, and the
/// runner token. Mirrors the Mise adapter's strip set without depending
/// on it; the orchestrator pins the two lists equal by test.
pub const STEP_CREDENTIAL_DENYLIST: [&str; 4] = [
    "MISE_GITHUB_TOKEN",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "ACTIONS_RUNTIME_TOKEN",
];

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

/// Reject denied credential keys in a step env map.
/// # Errors
pub fn reject_denied_step_keys(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for key in STEP_CREDENTIAL_DENYLIST {
        if env.contains_key(key) {
            return Err(RenderError::BadCommand(format!(
                "credential_step_env:{key}"
            )));
        }
    }
    Ok(())
}

/// Isolation keys forbidden in project-task step env (P07-7 hook escape).
///
/// The isolation quartet plus the install-disable pair. Trusted generated
/// steps carry these from the Mise adapter's single source; untrusted
/// project-task declarations must never smuggle them in to re-enable
/// hooks, config, or implicit installs. Mirrors the Mise adapter's
/// reserved set minus credentials (denied separately above) without
/// depending on it.
pub const STEP_ISOLATION_DENYLIST: [&str; 6] = [
    "MISE_NO_CONFIG",
    "MISE_NO_ENV",
    "MISE_NO_HOOKS",
    "MISE_LOCKFILE",
    "MISE_AUTO_INSTALL",
    "MISE_EXEC_AUTO_INSTALL",
];

/// Reject privileged isolation keys in a project-task step env map.
/// # Errors
pub fn reject_privileged_task_keys(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for key in STEP_ISOLATION_DENYLIST {
        if env.contains_key(key) {
            return Err(RenderError::BadCommand(format!(
                "privileged_task_env:{key}"
            )));
        }
    }
    Ok(())
}

/// Merge the triple over untrusted project-task declarations, refusing all privileged keys.
///
/// Same contract as [`checked_task_env`], plus isolation-key denial: a
/// hostile project task cannot re-enable hooks/config or smuggle
/// credentials through its declared env.
/// # Errors
pub fn checked_project_task_env(
    base: &BTreeMap<String, String>,
    rustup_home: &str,
    cargo_home: &str,
    toolchain: &str,
) -> Result<BTreeMap<String, String>, RenderError> {
    reject_privileged_task_keys(base)?;
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
        (TOOLCHAIN_HOME_KEYS[2], toolchain),
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
