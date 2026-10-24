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

use super::{EnvPolicy, IsolatedCommand, is_safe_credential_value};

/// Credential keys the private-fetch policy keeps (HD-7).
///
/// `VELNOR_PRIVATE_FETCH_TOKEN` is the GitHub App installation token that
/// reads private release assets, Maven bundles and git trees;
/// `VELNOR_NPM_READ_TOKEN` is the `GITHUB_TOKEN` `packages: read` value of the
/// single `bun install --ignore-scripts` registry phase. Both strip as
/// `*_TOKEN` credentials under every other policy. Mirrored by the renderer's
/// `STEP_PRIVATE_FETCH_ALLOWLIST`; the orchestrator pins the two equal by test.
pub const CREDENTIAL_ALLOWLIST_PRIVATE_FETCH: [&str; 2] =
    ["VELNOR_PRIVATE_FETCH_TOKEN", "VELNOR_NPM_READ_TOKEN"];

/// Exact fetch-redirect keys no inheriting child ever receives (HD-7).
///
/// Each can redirect a credentialed fetch to an attacker host or load an
/// attacker credential helper: an askpass program, git configuration
/// injected through the environment, or a registry override. Velnor's own
/// `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_NOSYSTEM` arrive through constructor-baked
/// extras after the strip. The pattern keys (`GIT_CONFIG_*`, `NPM_CONFIG_*`,
/// `npm_config_*`) are covered by [`is_fetch_redirect_key`]. Mirrored by the
/// renderer's `STEP_FETCH_REDIRECT_DENYLIST`; the orchestrator pins the two
/// equal by test.
pub const FETCH_REDIRECT_ENV_KEYS: [&str; 4] = [
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "GIT_CONFIG",
    "BUN_CONFIG_REGISTRY",
];

/// True for a fetch-redirect key: the exact keys, any `GIT_CONFIG_*`
/// (`GLOBAL`, `SYSTEM`, `NOSYSTEM`, `COUNT`, `KEY_n`, `VALUE_n`,
/// `PARAMETERS`), any `NPM_CONFIG_*`, or any `npm_config_*`.
#[must_use]
pub fn is_fetch_redirect_key(key: &str) -> bool {
    FETCH_REDIRECT_ENV_KEYS.contains(&key)
        || key.starts_with("GIT_CONFIG_")
        || key.starts_with("NPM_CONFIG_")
        || key.starts_with("npm_config_")
}

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

/// Parent keys a private-fetch child may copy: the lookup path and home that
/// `mise`, `git` and `cargo` need to find their tools. Nothing else ambient.
const PARENT_PASSTHROUGH: [&str; 3] = ["PATH", "HOME", "TMPDIR"];

/// Owned toolchain keys the generated step env supplies to the child: the
/// Mise isolation overlay, the install-disable pair and the owned tool homes.
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
    /// only git protocol, `PATH`/`HOME`/`TMPDIR` copied from the parent, the
    /// owned toolchain keys in `owned_env`, and, when given, exactly one
    /// allowlisted credential.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidStepInput`] for a relative or non-UTF-8
    /// configuration path, an `owned_env` key outside the owned toolchain set,
    /// or a credential value that is empty or contains anything but ASCII
    /// alphanumerics, `_`, `-` and `.`.
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
        })
    }
}
