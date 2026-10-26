// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::process::Command;

use super::private_fetch::{CREDENTIAL_ALLOWLIST_PRIVATE_FETCH, is_fetch_redirect_key};

pub const ISOLATION_ENV: [(&str, &str); 4] = [
    ("MISE_NO_CONFIG", "1"),
    ("MISE_NO_ENV", "1"),
    ("MISE_NO_HOOKS", "1"),
    ("MISE_LOCKFILE", "0"),
];

pub const NO_AUTO_INSTALL_ENV: [(&str, &str); 2] = [
    ("MISE_AUTO_INSTALL", "false"),
    ("MISE_EXEC_AUTO_INSTALL", "false"),
];

pub const MISE_RUSTUP_HOME_ENV: &str = "MISE_RUSTUP_HOME";

pub const MISE_CARGO_HOME_ENV: &str = "MISE_CARGO_HOME";

pub const RUSTUP_TOOLCHAIN_ENV: &str = "RUSTUP_TOOLCHAIN";

pub const TF_IN_AUTOMATION_ENV: &str = "TF_IN_AUTOMATION";
pub const TF_IN_AUTOMATION_ON: &str = "1";
pub const TF_INPUT_ENV: &str = "TF_INPUT";
pub const TF_INPUT_OFF: &str = "0";
pub const TF_DATA_DIR_ENV: &str = "TF_DATA_DIR";
pub const TF_CLI_CONFIG_FILE_ENV: &str = "TF_CLI_CONFIG_FILE";
pub const TF_PLUGIN_CACHE_DIR_ENV: &str = "TF_PLUGIN_CACHE_DIR";

pub const CREDENTIAL_ENV_KEYS: [&str; 9] = [
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

pub const ENDPOINT_ENV_KEYS: [&str; 2] = ["GH_HOST", "GH_CONFIG_DIR"];

#[must_use]
pub fn is_denied_endpoint_key(key: &str) -> bool {
    ENDPOINT_ENV_KEYS.contains(&key)
}

#[must_use]
pub fn is_denied_credential_key(key: &str) -> bool {
    CREDENTIAL_ENV_KEYS.contains(&key)
        || key.starts_with("CARGO_REGISTRIES_")
        || key.ends_with("_TOKEN")
}

pub const PROXY_ENV_KEYS: [&str; 8] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "all_proxy",
];

pub const CREDENTIAL_ALLOWLIST_BOOTSTRAP: [&str; 3] =
    ["MISE_GITHUB_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"];

pub const CREDENTIAL_ALLOWLIST_GH_API: [&str; 2] = ["GITHUB_TOKEN", "GH_TOKEN"];

const MISE_ALLOWED_EXTRAS: [&str; 2] = [MISE_RUSTUP_HOME_ENV, MISE_CARGO_HOME_ENV];

const TOFU_ALLOWED_EXTRAS: [&str; 2] = [TF_IN_AUTOMATION_ENV, TF_INPUT_ENV];

#[must_use]
pub fn is_reserved_env_key(key: &str) -> bool {
    if MISE_ALLOWED_EXTRAS.contains(&key) {
        return false;
    }
    if key.starts_with("MISE_") {
        return true;
    }
    if TOFU_ALLOWED_EXTRAS.contains(&key) {
        return false;
    }
    if key.starts_with("TF_") || key.starts_with("TOFU_") || key.starts_with("CHECKPOINT_") {
        return true;
    }
    is_denied_credential_key(key) || is_denied_endpoint_key(key)
}

fn is_stripped_ambient_key(key: &str) -> bool {
    key.starts_with("TF_") || key.starts_with("TOFU_") || key.starts_with("CHECKPOINT_")
}

#[must_use]
pub(crate) fn redact_env_for_debug(env: &[(OsString, OsString)]) -> Vec<(String, String)> {
    env.iter()
        .map(|(key, value)| {
            let name = key.to_string_lossy().into_owned();
            let shown = if velnor_actions_contract::is_secret_env_name(&name) {
                "<redacted>".to_owned()
            } else {
                value.to_string_lossy().into_owned()
            };
            (name, shown)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvPolicy {
    Bootstrap,
    GhApi,
    Verify,
    Discovery,
    GitHubToken,
    ReleaseSigner,
    RepoTask,
    QualifiedCheck,
    QualifiedProbe,
    QualifiedAcquisition,
    PrivateFetch,
}

impl EnvPolicy {
    #[must_use]
    pub const fn allowed_credentials(&self) -> &'static [&'static str] {
        match self {
            Self::Bootstrap => &CREDENTIAL_ALLOWLIST_BOOTSTRAP,
            Self::GhApi => &CREDENTIAL_ALLOWLIST_GH_API,
            Self::PrivateFetch => &CREDENTIAL_ALLOWLIST_PRIVATE_FETCH,
            Self::Verify
            | Self::Discovery
            | Self::GitHubToken
            | Self::ReleaseSigner
            | Self::RepoTask
            | Self::QualifiedCheck
            | Self::QualifiedProbe
            | Self::QualifiedAcquisition => &[],
        }
    }

    #[must_use]
    pub const fn inherits_parent(&self) -> bool {
        !matches!(
            self,
            Self::RepoTask
                | Self::GitHubToken
                | Self::ReleaseSigner
                | Self::QualifiedCheck
                | Self::QualifiedProbe
                | Self::QualifiedAcquisition
                | Self::PrivateFetch
        )
    }

    #[must_use]
    pub const fn allows_proxy_passthrough(&self) -> bool {
        matches!(
            self,
            Self::RepoTask
                | Self::ReleaseSigner
                | Self::QualifiedCheck
                | Self::QualifiedAcquisition
                | Self::PrivateFetch
        )
    }

    #[must_use]
    pub fn child_env(
        &self,
        parent: &[(OsString, OsString)],
        additions: &[(OsString, OsString)],
    ) -> Vec<(OsString, OsString)> {
        let mut env = Vec::with_capacity(parent.len() + additions.len());
        if self.inherits_parent() {
            let allowed = self.allowed_credentials();
            for (key, value) in parent {
                let name = key.to_string_lossy();
                let stripped = (is_denied_credential_key(&name)
                    && !allowed.contains(&name.as_ref()))
                    || ENDPOINT_ENV_KEYS.contains(&name.as_ref())
                    || is_fetch_redirect_key(&name)
                    || is_stripped_ambient_key(&name);
                if !stripped {
                    env.push((key.clone(), value.clone()));
                }
            }
        } else {
            env.extend(super::cleared_parent_passthrough(*self, parent));
        }
        env.extend(additions.iter().cloned());
        env
    }
}

#[must_use]
pub fn proxy_passthrough(parent: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
    let mut kept = Vec::new();
    for key in PROXY_ENV_KEYS {
        for (name, value) in parent {
            if name == key {
                kept.push((name.clone(), value.clone()));
            }
        }
    }
    kept
}

pub(crate) fn strip_credentials(command: &mut Command, policy: EnvPolicy) {
    let allowed = policy.allowed_credentials();
    for key in CREDENTIAL_ENV_KEYS {
        if !allowed.contains(&key) {
            command.env_remove(key);
        }
    }
    for key in ENDPOINT_ENV_KEYS {
        command.env_remove(key);
    }
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if is_stripped_ambient_key(&name)
            || is_fetch_redirect_key(&name)
            || (is_denied_credential_key(&name)
                && !CREDENTIAL_ENV_KEYS.contains(&name.as_ref())
                && !allowed.contains(&name.as_ref()))
        {
            command.env_remove(key);
        }
    }
}

#[must_use]
pub fn toolchain_env(rustup: &str, cargo: &str, toolchain: &str) -> Vec<(OsString, OsString)> {
    [
        (MISE_RUSTUP_HOME_ENV, rustup),
        (MISE_CARGO_HOME_ENV, cargo),
        (RUSTUP_TOOLCHAIN_ENV, toolchain),
        ("RUSTUP_HOME", rustup),
        ("CARGO_HOME", cargo),
    ]
    .iter()
    .map(|(key, value)| (OsString::from(key), OsString::from(value)))
    .collect()
}

pub(crate) fn pairs_of<const N: usize>(table: &[(&str, &str); N]) -> Vec<(OsString, OsString)> {
    table
        .iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect()
}
