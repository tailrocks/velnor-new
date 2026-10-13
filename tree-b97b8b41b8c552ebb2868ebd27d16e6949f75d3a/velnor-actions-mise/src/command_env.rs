//! Environment policy for spawned children: reserved keys, isolation
//! overlay, and the Velnor-owned toolchain environment.
//!
//! Declared from `command.rs` (`#[path]`, no `lib.rs` edit); `command.rs`
//! re-exports the public surface so `command::X` paths keep working.

use std::ffi::OsString;

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

/// Whether a key is reserved: isolation, install disable, or credentials.
/// Credentials are `MISE_GITHUB_TOKEN` plus the `GITHUB_TOKEN`/`GH_TOKEN`
/// aliases and `ACTIONS_RUNTIME_TOKEN`.
#[must_use]
pub fn is_reserved_env_key(key: &str) -> bool {
    ISOLATION_ENV.iter().any(|(own, _)| *own == key)
        || NO_AUTO_INSTALL_ENV.iter().any(|(own, _)| *own == key)
        || [
            "MISE_GITHUB_TOKEN",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "ACTIONS_RUNTIME_TOKEN",
        ]
        .contains(&key)
}

/// Which parent environment a child may see: bootstrap, verify, and
/// discovery inherit; repo-task spawns from `env_clear` plus an explicit list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvPolicy {
    /// Trusted tool download: inherits parent env plus isolation overlay.
    Bootstrap,
    /// Trusted evidence validation: inherits parent env plus isolation overlay.
    Verify,
    /// Read-only discovery probes: inherits parent env plus isolation overlay.
    Discovery,
    /// Repository task execution: cleared env plus declared inputs only.
    RepoTask,
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
