//! Private-fetch spawn constructor (HD-7): cleared environment, baked git
//! isolation, and at most one allowlisted credential.
//!
//! Declared from `command.rs` (`#[path]`, no `lib.rs` edit). The constructor
//! is the only way to reach [`EnvPolicy::PrivateFetch`]; `with_env` can never
//! carry a `*_TOKEN` or a git-config key, so every private-fetch value arrives
//! through here.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::MiseError;

use super::{CREDENTIAL_ALLOWLIST_PRIVATE_FETCH, EnvPolicy, IsolatedCommand};

/// Which allowlisted credential a private-fetch child receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateFetchCredential {
    /// GitHub App installation token (`contents: read`) as `VELNOR_PRIVATE_FETCH_TOKEN`.
    AppInstallation,
    /// `GITHUB_TOKEN` `packages: read` value as `VELNOR_NPM_READ_TOKEN`.
    NpmRegistryRead,
}

impl PrivateFetchCredential {
    /// Environment key carrying this credential.
    #[must_use]
    pub const fn env_key(self) -> &'static str {
        match self {
            Self::AppInstallation => CREDENTIAL_ALLOWLIST_PRIVATE_FETCH[0],
            Self::NpmRegistryRead => CREDENTIAL_ALLOWLIST_PRIVATE_FETCH[1],
        }
    }
}

/// Fixed environment baked into every private-fetch child, apart from the
/// Velnor-owned `GIT_CONFIG_GLOBAL` path.
const BAKED_FETCH_ENV: [(&str, &str); 4] = [
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_ALLOW_PROTOCOL", "https"),
    ("CARGO_NET_GIT_FETCH_WITH_CLI", "true"),
];

impl IsolatedCommand {
    /// Build a private-fetch child under [`EnvPolicy::PrivateFetch`].
    ///
    /// The child starts from an empty environment plus proxy passthrough. It
    /// receives the Velnor-owned git configuration file as `GIT_CONFIG_GLOBAL`,
    /// system git configuration disabled, no interactive prompts, https as the
    /// only git protocol, and, when given, exactly one allowlisted credential.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidStepInput`] for a relative or non-UTF-8
    /// configuration path, or a credential value that is empty or contains
    /// anything but ASCII alphanumerics, `_`, `-` and `.`.
    pub fn private_fetch(
        program: &str,
        args: Vec<OsString>,
        cwd: PathBuf,
        git_config_global: &Path,
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
        if let Some((kind, value)) = credential {
            if value.is_empty()
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
            {
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
        })
    }
}
