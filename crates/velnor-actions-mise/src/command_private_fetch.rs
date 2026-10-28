// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::MiseError;

use super::{EnvPolicy, IsolatedCommand, is_safe_credential_value};

pub const CREDENTIAL_ALLOWLIST_PRIVATE_FETCH: [&str; 2] =
    ["VELNOR_PRIVATE_FETCH_TOKEN", "VELNOR_NPM_READ_TOKEN"];

pub const FETCH_REDIRECT_ENV_KEYS: [&str; 4] = [
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "GIT_CONFIG",
    "BUN_CONFIG_REGISTRY",
];

#[must_use]
pub fn is_fetch_redirect_key(key: &str) -> bool {
    FETCH_REDIRECT_ENV_KEYS.contains(&key)
        || key.starts_with("GIT_CONFIG_")
        || key.starts_with("NPM_CONFIG_")
        || key.starts_with("npm_config_")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateFetchCredential {
    AppInstallation,
    NpmRegistryRead,
}

impl PrivateFetchCredential {
    #[must_use]
    pub const fn env_key(self) -> &'static str {
        match self {
            Self::AppInstallation => CREDENTIAL_ALLOWLIST_PRIVATE_FETCH[0],
            Self::NpmRegistryRead => CREDENTIAL_ALLOWLIST_PRIVATE_FETCH[1],
        }
    }
}

const PARENT_PASSTHROUGH: [&str; 3] = ["PATH", "HOME", "TMPDIR"];

const OWNED_ENV_KEYS: [&str; 10] = [
    "MISE_NO_CONFIG",
    "MISE_NO_ENV",
    "MISE_NO_HOOKS",
    "MISE_LOCKFILE",
    "MISE_AUTO_INSTALL",
    "MISE_EXEC_AUTO_INSTALL",
    "MISE_RUSTUP_HOME",
    "MISE_CARGO_HOME",
    "RUSTUP_TOOLCHAIN",
    "BUN_INSTALL_CACHE_DIR",
];

const BAKED_FETCH_ENV: [(&str, &str); 4] = [
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_ALLOW_PROTOCOL", "https"),
    ("CARGO_NET_GIT_FETCH_WITH_CLI", "true"),
];

impl IsolatedCommand {
    pub fn private_fetch(
        program: &str,
        args: Vec<OsString>,
        cwd: PathBuf,
        git_config_global: &Path,
        owned_env: &[(String, String)],
        credential: Option<(PrivateFetchCredential, &str)>,
    ) -> Result<Self, MiseError> {
        let invalid = |field: &str, value: &str| MiseError::InvalidStepInput {
            field: field.to_owned(),
            value: value.to_owned(),
        };
        if !git_config_global.is_absolute() || git_config_global.to_str().is_none() {
            return Err(invalid("git_config_global", "must_be_absolute_utf8"));
        }
        let mut extra_env: Vec<(OsString, OsString)> = BAKED_FETCH_ENV
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect();
        extra_env.push((
            OsString::from("GIT_CONFIG_GLOBAL"),
            git_config_global.as_os_str().to_owned(),
        ));
        for key in PARENT_PASSTHROUGH {
            if let Some(value) = std::env::var_os(key) {
                extra_env.push((OsString::from(key), value));
            }
        }
        for (key, value) in owned_env {
            if !OWNED_ENV_KEYS.contains(&key.as_str()) {
                return Err(invalid(key, "not_an_owned_toolchain_key"));
            }
            extra_env.push((OsString::from(key), OsString::from(value)));
        }
        if let Some((kind, value)) = credential {
            if !is_safe_credential_value(value) {
                return Err(invalid(kind.env_key(), "credential_rejected"));
            }
            extra_env.push((OsString::from(kind.env_key()), OsString::from(value)));
        }
        Ok(Self {
            program: OsString::from(program),
            args,
            cwd: Some(cwd),
            extra_env,
            policy: EnvPolicy::PrivateFetch,
            operator_guarded: false,
        })
    }
}
