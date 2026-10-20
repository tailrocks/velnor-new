//! Private-fetch env contract mirrored from the Mise adapter (HD-7).
//!
//! Declared from `toolchain_env.rs` (`#[path]`, no `lib.rs` edit).

/// Credential keys only a private-fetch step may bind (HD-7).
///
/// Mirrors the Mise adapter's `CREDENTIAL_ALLOWLIST_PRIVATE_FETCH`
/// element-for-element without depending on it; the orchestrator pins the
/// two lists equal by test.
pub const STEP_PRIVATE_FETCH_ALLOWLIST: [&str; 2] =
    ["VELNOR_PRIVATE_FETCH_TOKEN", "VELNOR_NPM_READ_TOKEN"];

/// Exact fetch-redirect keys no rendered task env may carry (HD-7).
///
/// Mirrors the Mise adapter's `FETCH_REDIRECT_ENV_KEYS`; the pattern
/// families are covered by [`is_fetch_redirect_key`]. The orchestrator pins
/// the two lists and predicates equal by test.
pub const STEP_FETCH_REDIRECT_DENYLIST: [&str; 4] = [
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "GIT_CONFIG",
    "BUN_CONFIG_REGISTRY",
];

/// True for a fetch-redirect key: the exact keys, any `GIT_CONFIG_*`, any
/// `NPM_CONFIG_*`, or any `npm_config_*`.
#[must_use]
pub fn is_fetch_redirect_key(key: &str) -> bool {
    STEP_FETCH_REDIRECT_DENYLIST.contains(&key)
        || key.starts_with("GIT_CONFIG_")
        || key.starts_with("NPM_CONFIG_")
        || key.starts_with("npm_config_")
}
