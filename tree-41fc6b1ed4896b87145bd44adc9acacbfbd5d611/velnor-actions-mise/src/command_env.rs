//! Environment policy for spawned children: reserved keys, isolation
//! overlay, and the Velnor-owned toolchain environment.
//!
//! Declared from `command.rs` (`#[path]`, no `lib.rs` edit); `command.rs`
//! re-exports the public surface so `command::X` paths keep working.

use std::ffi::OsString;
use std::process::Command;

/// Isolation environment applied to every spawned process.
pub const ISOLATION_ENV: [(&str, &str); 4] = [
    ("MISE_NO_CONFIG", "1"),
    ("MISE_NO_ENV", "1"),
    ("MISE_NO_HOOKS", "1"),
    ("MISE_LOCKFILE", "0"),
];

/// Environment disabling implicit tool installation for verification runs.
pub const NO_AUTO_INSTALL_ENV: [(&str, &str); 2] = [
    ("MISE_AUTO_INSTALL", "false"),
    ("MISE_EXEC_AUTO_INSTALL", "false"),
];

/// Environment name for the Velnor-owned mise Rust home.
pub const MISE_RUSTUP_HOME_ENV: &str = "MISE_RUSTUP_HOME";

/// Environment name for the Velnor-owned mise Cargo home.
pub const MISE_CARGO_HOME_ENV: &str = "MISE_CARGO_HOME";

/// Environment name selecting the exact Rust toolchain for Cargo runs.
pub const RUSTUP_TOOLCHAIN_ENV: &str = "RUSTUP_TOOLCHAIN";

/// Credential keys that must never reach a task environment.
///
/// Static tokens (`MISE_GITHUB_TOKEN`, `GITHUB_TOKEN`/`GH_TOKEN`,
/// `ACTIONS_RUNTIME_TOKEN`, `CARGO_REGISTRY_TOKEN`) plus the OIDC
/// token-request pair: the URL mints tokens, so it is as sensitive
/// as the token itself.
pub const CREDENTIAL_ENV_KEYS: [&str; 7] = [
    "MISE_GITHUB_TOKEN",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "ACTIONS_RUNTIME_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_URL",
    "CARGO_REGISTRY_TOKEN",
];

/// Proxy/network keys a repo-task child inherits from its parent.
///
/// Cleared repo-task children keep legitimate proxy configuration and
/// nothing else ambient: these eight spellings (both cases, the
/// conventional set every HTTP client honors) pass through, while
/// every other parent key — credentials included — stays behind.
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

/// Credential keys the bootstrap policy keeps for tool download.
///
/// Mise authenticates release downloads against the GitHub trio; the
/// runtime token, the OIDC pair, and the registry token never serve
/// downloads and are stripped even here.
pub const CREDENTIAL_ALLOWLIST_BOOTSTRAP: [&str; 3] =
    ["MISE_GITHUB_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"];

/// Credential keys the baseline policy keeps for `gh` API reads.
///
/// `gh` authenticates with the ambient CI identity only; the
/// download token, the runtime token, the OIDC pair, and the
/// registry token are stripped.
pub const CREDENTIAL_ALLOWLIST_BASELINE: [&str; 2] = ["GITHUB_TOKEN", "GH_TOKEN"];

/// Whether a key is reserved: isolation, install disable, or credentials.
/// Credentials are [`CREDENTIAL_ENV_KEYS`]; note `CARGO_REGISTRY_TOKEN`
/// (a repo task carrying it would silently disable trusted publishing).
#[must_use]
pub fn is_reserved_env_key(key: &str) -> bool {
    ISOLATION_ENV.iter().any(|(own, _)| *own == key)
        || NO_AUTO_INSTALL_ENV.iter().any(|(own, _)| *own == key)
        || CREDENTIAL_ENV_KEYS.contains(&key)
}

/// Redact secret-looking values for `Debug`: names stay, values become
/// `<redacted>` when [`velnor_actions_contract::is_secret_env_name`]
/// matches. Non-secret pairs render unchanged.
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

/// Which parent environment a child may see: every policy inherits
/// at most what its purpose needs, and no two purposes share the
/// same token-bearing environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvPolicy {
    /// Trusted tool download: inherits minus everything outside the
    /// download allowlist, plus the isolation overlay.
    Bootstrap,
    /// Exact-base baseline lookup via `gh`: inherits minus everything
    /// outside the baseline allowlist, plus the isolation overlay.
    Baseline,
    /// Trusted evidence validation (offline): inherits minus every
    /// credential key, plus the isolation overlay.
    Verify,
    /// Read-only local discovery probes: inherits minus every
    /// credential key, plus the isolation overlay.
    Discovery,
    /// Repository task execution: cleared env plus proxy passthrough,
    /// the isolation overlay, and declared inputs only.
    RepoTask,
}

impl EnvPolicy {
    /// Credential keys this policy keeps; every other
    /// [`CREDENTIAL_ENV_KEYS`] entry is stripped before spawn.
    #[must_use]
    pub const fn allowed_credentials(&self) -> &'static [&'static str] {
        match self {
            Self::Bootstrap => &CREDENTIAL_ALLOWLIST_BOOTSTRAP,
            Self::Baseline => &CREDENTIAL_ALLOWLIST_BASELINE,
            Self::Verify | Self::Discovery | Self::RepoTask => &[],
        }
    }

    /// Whether the child inherits the parent environment (filtered).
    /// Only [`Self::RepoTask`] spawns cleared.
    #[must_use]
    pub const fn inherits_parent(&self) -> bool {
        !matches!(self, Self::RepoTask)
    }

    /// Full child environment over an explicit parent snapshot.
    ///
    /// The pure contract behind the spawner: inheriting policies keep
    /// the parent minus stripped credentials, repo-task keeps the
    /// proxy passthrough only, and every policy appends `additions`
    /// (the isolation overlay plus validated extras) last.
    #[must_use]
    pub fn child_env(
        &self,
        parent: &[(OsString, OsString)],
        additions: &[(OsString, OsString)],
    ) -> Vec<(OsString, OsString)> {
        let mut env = Vec::with_capacity(parent.len() + additions.len());
        if *self == Self::RepoTask {
            env.extend(proxy_passthrough(parent));
        } else {
            let allowed = self.allowed_credentials();
            for (key, value) in parent {
                let name = key.to_string_lossy();
                let stripped = CREDENTIAL_ENV_KEYS.contains(&name.as_ref())
                    && !allowed.contains(&name.as_ref());
                if !stripped {
                    env.push((key.clone(), value.clone()));
                }
            }
        }
        env.extend(additions.iter().cloned());
        env
    }
}

/// Parent proxy entries in [`PROXY_ENV_KEYS`] order (P07-2).
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

/// Remove every credential key the policy does not allow (P07-1).
pub(crate) fn strip_credentials(command: &mut Command, policy: EnvPolicy) {
    let allowed = policy.allowed_credentials();
    for key in CREDENTIAL_ENV_KEYS {
        if !allowed.contains(&key) {
            command.env_remove(key);
        }
    }
}

/// Velnor-owned toolchain environment for one Cargo invocation.
#[must_use]
pub fn toolchain_env(rustup: &str, cargo: &str, toolchain: &str) -> Vec<(OsString, OsString)> {
    [
        (MISE_RUSTUP_HOME_ENV, rustup),
        (MISE_CARGO_HOME_ENV, cargo),
        (RUSTUP_TOOLCHAIN_ENV, toolchain),
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
