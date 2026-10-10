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

/// Tofu automation-marker env key (allowed extra; T03 pair).
///
/// Mirrors the tofu adapter's key without depending on it; the
/// orchestrator pins the two equal by test.
pub const TF_IN_AUTOMATION_ENV: &str = "TF_IN_AUTOMATION";
/// Tofu automation marker enabled.
pub const TF_IN_AUTOMATION_ON: &str = "1";
/// Tofu interactive-input env key (allowed extra; T03 pair).
pub const TF_INPUT_ENV: &str = "TF_INPUT";
/// Tofu interactive input disabled.
pub const TF_INPUT_OFF: &str = "0";
/// Tofu isolated data-dir env key (constructor-baked only).
pub const TF_DATA_DIR_ENV: &str = "TF_DATA_DIR";
/// Tofu isolated CLI-config env key (constructor-baked only).
pub const TF_CLI_CONFIG_FILE_ENV: &str = "TF_CLI_CONFIG_FILE";
/// Tofu plugin-cache env key (constructor-baked only; T21 transport).
///
/// Mirrors the tofu adapter's key without depending on it; the
/// orchestrator pins the two equal by test.
pub const TF_PLUGIN_CACHE_DIR_ENV: &str = "TF_PLUGIN_CACHE_DIR";

/// Credential keys that must never reach a task environment.
///
/// Static tokens (`MISE_GITHUB_TOKEN`, `GITHUB_TOKEN`/`GH_TOKEN`,
/// `ACTIONS_RUNTIME_TOKEN`, `CARGO_REGISTRY_TOKEN`, the npm pair) plus
/// the OIDC token-request pair: the URL mints tokens, so it is as
/// sensitive as the token itself. Mirrors the renderer's scrub list
/// element-for-element; the orchestrator pins the two equal by test.
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

/// Endpoint-selector keys no spawned child ever inherits.
///
/// `GH_HOST` would reroute `gh` — and any kept token — to an
/// attacker host; `GH_CONFIG_DIR` would load attacker-controlled
/// auth. Stripped for every policy: this tool only talks to
/// `github.com`. (`GH_ENTERPRISE_TOKEN` already strips as a
/// `*_TOKEN` credential unless a policy allowlists it, and none
/// does.) Mirrored by the renderer's endpoint denylist; the
/// orchestrator pins the two lists equal by test.
pub const ENDPOINT_ENV_KEYS: [&str; 2] = ["GH_HOST", "GH_CONFIG_DIR"];

/// True for an endpoint-selector key: never inherited, never
/// overridable, never emitted into rendered step env.
#[must_use]
pub fn is_denied_endpoint_key(key: &str) -> bool {
    ENDPOINT_ENV_KEYS.contains(&key)
}

/// True for a credential-shaped env key: the nine known names, any
/// `CARGO_REGISTRIES_*` entry, or any `*_TOKEN` name.
///
/// Mirrors the renderer's predicate exactly (same three clauses, same
/// order); the orchestrator parity test asserts agreement on a corpus
/// covering exact, prefix, suffix, and clean keys.
#[must_use]
pub fn is_denied_credential_key(key: &str) -> bool {
    CREDENTIAL_ENV_KEYS.contains(&key)
        || key.starts_with("CARGO_REGISTRIES_")
        || key.ends_with("_TOKEN")
}

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

/// Velnor-owned `MISE_*` keys that declared extras may still carry.
///
/// The owned tool homes flow through [`crate::command::IsolatedCommand::with_env`]
/// as declared extras, so they stay allowed; every other `MISE_*` name is
/// reserved. Isolation ([`ISOLATION_ENV`]) and install-disable
/// ([`NO_AUTO_INSTALL_ENV`]) keys arrive via the fixed overlays only, never
/// as caller extras, so the prefix rule keeps them reserved without
/// enumerating them.
const MISE_ALLOWED_EXTRAS: [&str; 2] = [MISE_RUSTUP_HOME_ENV, MISE_CARGO_HOME_ENV];

/// Tofu keys that declared extras may still carry: the harmless T03
/// automation pair, which proposal env threads through step extras.
/// Every other `TF_*`/`TOFU_*` name arrives via the tofu spawn
/// constructor only, never as caller extras.
const TOFU_ALLOWED_EXTRAS: [&str; 2] = [TF_IN_AUTOMATION_ENV, TF_INPUT_ENV];

/// Whether a key is reserved: the `MISE_*` prefix (minus the owned-homes
/// allowlist), the `TF_*`/`TOFU_*` prefixes (minus the automation-pair
/// allowlist), the `CHECKPOINT_*` prefix, credentials, or endpoint
/// selectors. Credentials are pattern-matched
/// ([`is_denied_credential_key`]); note `CARGO_REGISTRY_TOKEN` (a repo
/// task carrying it would silently disable trusted publishing) and its
/// `CARGO_REGISTRIES_*` siblings. The prefix rules close the override
/// hole whole: any `MISE_*` name mise reads (`MISE_ENV`,
/// `MISE_CONFIG_FILE`, `MISE_TRUSTED_CONFIG_PATHS`, `MISE_DATA_DIR`,
/// ...) and any `TF_*` name tofu reads (`TF_VAR_*`, `TF_CLI_ARGS_*`,
/// `TF_TOKEN_*`, `TF_WORKSPACE`, `TF_LOG`, ...) fail loud instead of
/// only the enumerated few.
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

/// True for ambient-only tofu/checkpoint families: stripped from every
/// inherited parent before spawn (Velnor's own pairs arrive via the
/// fixed overlays after the strip, so constructor-set values survive).
fn is_stripped_ambient_key(key: &str) -> bool {
    key.starts_with("TF_") || key.starts_with("TOFU_") || key.starts_with("CHECKPOINT_")
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
    /// download allowlist, plus the full isolation overlay. Installs
    /// never load repo config (`MISE_NO_CONFIG=1` always); explicit
    /// `tool@exact` specs are the sole version authority.
    Bootstrap,
    /// Exact-base baseline lookup via `gh`: inherits minus everything
    /// outside the baseline allowlist, plus the isolation overlay.
    Baseline,
    /// Trusted evidence validation (offline): inherits minus every
    /// credential key, plus the isolation overlay.
    Verify,
    /// Pinned MBX execution: inherits the verification environment while
    /// removing Mise's cargo command-wrapper path so MBX resolves the
    /// selected real Rust toolchain binaries itself.
    Mbx,
    /// Read-only local discovery probes: inherits minus every
    /// credential key, plus the isolation overlay.
    Discovery,
    /// Repository task execution: cleared env plus proxy passthrough,
    /// the isolation overlay, and declared inputs only.
    RepoTask,
    /// Qualified task-only config visibility with owned homes, cleared env,
    /// and proxy passthrough.
    QualifiedCheck,
    /// Read-only qualified probes: explicit environment only, no overlays.
    QualifiedProbe,
    /// Qualified acquisition: owned environment, proxy passthrough, and fixed overlay.
    QualifiedAcquisition,
}

impl EnvPolicy {
    /// Credential keys this policy keeps; every other
    /// [`CREDENTIAL_ENV_KEYS`] entry is stripped before spawn.
    #[must_use]
    pub const fn allowed_credentials(&self) -> &'static [&'static str] {
        match self {
            Self::Bootstrap => &CREDENTIAL_ALLOWLIST_BOOTSTRAP,
            Self::Baseline => &CREDENTIAL_ALLOWLIST_BASELINE,
            Self::Verify
            | Self::Mbx
            | Self::Discovery
            | Self::RepoTask
            | Self::QualifiedCheck
            | Self::QualifiedProbe
            | Self::QualifiedAcquisition => &[],
        }
    }

    /// Whether the child inherits the parent environment (filtered).
    /// Repository tasks and qualified policies spawn cleared.
    #[must_use]
    pub const fn inherits_parent(&self) -> bool {
        !matches!(
            self,
            Self::RepoTask
                | Self::QualifiedCheck
                | Self::QualifiedProbe
                | Self::QualifiedAcquisition
        )
    }

    /// Whether a cleared child may inherit only the ambient proxy keys.
    #[must_use]
    pub const fn allows_proxy_passthrough(&self) -> bool {
        matches!(
            self,
            Self::RepoTask | Self::QualifiedCheck | Self::QualifiedAcquisition
        )
    }

    /// Full child environment over an explicit parent snapshot.
    ///
    /// The pure contract behind the spawner: inheriting policies keep
    /// the parent minus stripped credentials, endpoints, and
    /// tofu/checkpoint ambient families; cleared tasks and acquisitions keep
    /// only proxy passthrough; and every policy appends `additions` (the
    /// isolation overlay plus validated extras) last.
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
                    || is_stripped_ambient_key(&name);
                if !stripped {
                    env.push((key.clone(), value.clone()));
                }
            }
        } else if self.allows_proxy_passthrough() {
            env.extend(proxy_passthrough(parent));
        }
        env.extend(additions.iter().cloned());
        if matches!(self, Self::Mbx) {
            sanitize_mise_command_wrapper_path(&mut env, parent);
        }
        env
    }
}

/// Remove only the caller's canonical Mise command-wrapper directory from the
/// last PATH assignment while preserving Rustup and unrelated path entries.
/// MBX invokes Cargo internally; exposing Mise's outer command-wrapper entry
/// makes `mise exec ... -- cargo` fail its shim check instead of reaching the
/// Rustup-selected Cargo binary.
fn sanitize_mise_command_wrapper_path(
    env: &mut Vec<(OsString, OsString)>,
    parent: &[(OsString, OsString)],
) {
    let Some(wrapper_dir) = mise_command_wrapper_dir(parent) else {
        return;
    };
    let Some((_, path)) = env.iter().rev().find(|(key, _)| key == "PATH") else {
        return;
    };
    let entries = std::env::split_paths(path)
        .filter(|entry| !same_path(entry, &wrapper_dir))
        .collect::<Vec<_>>();
    let Ok(path) = std::env::join_paths(entries) else {
        // A malformed PATH must not reintroduce the rejected shim. Omitting
        // it makes MBX fail closed when it tries to resolve its compiler.
        env.retain(|(key, _)| key != "PATH");
        return;
    };
    env.retain(|(key, _)| key != "PATH");
    env.push((OsString::from("PATH"), path));
}

fn mise_command_wrapper_dir(parent: &[(OsString, OsString)]) -> Option<std::path::PathBuf> {
    let mise_data_dir = parent
        .iter()
        .rev()
        .find(|(key, _)| key == "MISE_DATA_DIR")
        .map(|(_, value)| std::path::PathBuf::from(value));
    // Keep the default path tied to the caller's home rather than the
    // isolated MISE_DATA_DIR added for the child.
    let base = mise_data_dir.or_else(|| {
        parent
            .iter()
            .rev()
            .find(|(key, _)| key == "HOME")
            .map(|(_, home)| std::path::PathBuf::from(home).join(".local/share/mise"))
    })?;
    if !base.is_absolute() {
        return None;
    }
    Some(base.join("command-wrappers").join("bin"))
}

fn same_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    let left = left.components().collect::<Vec<_>>();
    let right = right.components().collect::<Vec<_>>();
    left.len() == right.len()
        && left.iter().zip(right.iter()).all(|(left, right)| {
            let left = left.as_os_str().to_string_lossy();
            let right = right.as_os_str().to_string_lossy();
            if cfg!(windows) || cfg!(target_os = "macos") {
                left.eq_ignore_ascii_case(&right)
            } else {
                left == right
            }
        })
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
///
/// The nine known names strip unconditionally; pattern-shaped names
/// (`CARGO_REGISTRIES_*`, `*_TOKEN`) and the tofu/checkpoint ambient
/// families (`TF_*`, `TOFU_*`, `CHECKPOINT_*`) strip by scanning the
/// live parent environment, since no fixed list can enumerate them.
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
            || (is_denied_credential_key(&name)
                && !CREDENTIAL_ENV_KEYS.contains(&name.as_ref())
                && !allowed.contains(&name.as_ref()))
        {
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
