// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::RenderError;

#[path = "toolchain_env_homes.rs"]
mod homes;
pub use homes::{
    SHARED_CARGO_HOME, SHARED_OBSERVATION_ROOT, SHARED_OWNED_ROOT, SHARED_RUSTUP_HOME,
    SHARED_SOURCE_ROOT, SHARED_TOOL_ROOT, SHARED_TRANSPORT_ROOT, SHARED_WORKSPACE_ROOT,
    SharedRunnerHomes, TOOLCHAIN_HOME_KEYS, check_toolchain_homes, checked_project_task_env,
    checked_task_env, reject_conflicting_home_assignments, with_toolchain_homes,
};

pub const STEP_CREDENTIAL_DENYLIST: [&str; 9] = [
    "MISE_GITHUB_TOKEN",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "ACTIONS_RUNTIME_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_URL",
    "CARGO_REGISTRY_TOKEN",
    "NPM_TOKEN",
    "NODE_AUTH_TOKEN",
];

pub const CARGO_AUTO_CLEAN_ENV: (&str, &str) = ("CARGO_CACHE_AUTO_CLEAN_FREQUENCY", "never");

pub const STEP_ENDPOINT_DENYLIST: [&str; 2] = ["GH_HOST", "GH_CONFIG_DIR"];

#[path = "toolchain_env_fetch.rs"]
mod fetch;
pub use fetch::*;
#[must_use]
pub fn is_denied_credential_key(key: &str) -> bool {
    STEP_CREDENTIAL_DENYLIST.contains(&key)
        || key.starts_with("CARGO_REGISTRIES_")
        || key.ends_with("_TOKEN")
}

#[must_use]
pub fn is_denied_endpoint_key(key: &str) -> bool {
    STEP_ENDPOINT_DENYLIST.contains(&key)
}

pub const STEP_TF_DENYLIST_PREFIXES: [&str; 2] = ["TF_", "CHECKPOINT_"];

pub const STEP_TF_ALLOWLIST: [&str; 2] = ["TF_IN_AUTOMATION", "TF_INPUT"];

#[must_use]
pub fn is_denied_tf_key(key: &str) -> bool {
    if STEP_TF_ALLOWLIST.contains(&key) {
        return false;
    }
    STEP_TF_DENYLIST_PREFIXES
        .iter()
        .any(|prefix| key.starts_with(prefix))
}

pub fn reject_denied_step_keys(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for key in env.keys() {
        if is_denied_credential_key(key) {
            return Err(RenderError::BadCommand(format!(
                "credential_step_env:{key}"
            )));
        }
        if is_denied_endpoint_key(key) || is_fetch_redirect_key(key) {
            return Err(RenderError::BadCommand(format!("endpoint_step_env:{key}")));
        }
    }
    Ok(())
}

#[must_use]
pub fn credential_scrub() -> BTreeMap<String, String> {
    STEP_CREDENTIAL_DENYLIST
        .iter()
        .chain(STEP_ENDPOINT_DENYLIST.iter())
        .map(|key| ((*key).to_owned(), String::new()))
        .collect()
}

#[must_use]
pub fn with_credential_scrub(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut scrubbed = env.clone();
    scrubbed.extend(credential_scrub());
    scrubbed
}

pub const CREDENTIAL_UNSET_VARS: [&str; 8] = [
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_URL",
    "ACTIONS_RUNTIME_TOKEN",
    "GITHUB_TOKEN",
    "MISE_GITHUB_TOKEN",
    "GH_TOKEN",
    "GH_HOST",
    "GH_CONFIG_DIR",
];

#[must_use]
pub fn credential_unset_prelude() -> String {
    format!("unset {};", CREDENTIAL_UNSET_VARS.join(" "))
}

#[must_use]
pub fn with_credential_unset_script(script: &str) -> String {
    format!("{} {script}", credential_unset_prelude())
}

#[must_use]
pub fn with_env_unset_argv(argv: &[String]) -> Vec<String> {
    let mut unset = Vec::with_capacity(argv.len() + 2 * CREDENTIAL_UNSET_VARS.len() + 1);
    unset.push("env".to_owned());
    for var in CREDENTIAL_UNSET_VARS {
        unset.push("-u".to_owned());
        unset.push(var.to_owned());
    }
    unset.extend(argv.iter().cloned());
    unset
}

#[must_use]
pub(crate) fn unset_prefix_len(argv: &[String]) -> usize {
    if argv.first().is_none_or(|arg| arg != "env") {
        return 0;
    }
    let mut len = 1;
    while argv.len() >= len + 2
        && argv[len] == "-u"
        && CREDENTIAL_UNSET_VARS.contains(&argv[len + 1].as_str())
    {
        len += 2;
    }
    len
}

pub const STEP_ISOLATION_DENYLIST: [&str; 6] = [
    "MISE_NO_CONFIG",
    "MISE_NO_ENV",
    "MISE_NO_HOOKS",
    "MISE_LOCKFILE",
    "MISE_AUTO_INSTALL",
    "MISE_EXEC_AUTO_INSTALL",
];

pub const MISE_STATIC_ENV: [(&str, &str); 6] = [
    ("MISE_AUTO_INSTALL", "false"),
    ("MISE_EXEC_AUTO_INSTALL", "false"),
    ("MISE_LOCKFILE", "0"),
    ("MISE_NO_CONFIG", "1"),
    ("MISE_NO_ENV", "1"),
    ("MISE_NO_HOOKS", "1"),
];

#[must_use]
pub fn workflow_level_env() -> BTreeMap<String, String> {
    let mut map = credential_scrub();
    for (key, value) in MISE_STATIC_ENV {
        map.insert(key.to_owned(), value.to_owned());
    }
    map
}

#[must_use]
pub fn job_level_env() -> BTreeMap<String, String> {
    workflow_level_env()
}

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

pub fn reject_denied_tf_keys(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for key in env.keys() {
        if is_denied_tf_key(key) {
            return Err(RenderError::BadCommand(format!("tf_task_env:{key}")));
        }
    }
    Ok(())
}
