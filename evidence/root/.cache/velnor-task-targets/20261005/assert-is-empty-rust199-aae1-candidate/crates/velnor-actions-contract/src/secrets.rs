//! Secret-name screening for identity inputs (cache §1).
//!
//! Secrets MUST never enter an identity or report. Values that affect
//! behavior are declared by name; names that look like credentials are
//! rejected fail-closed before they can reach a digest.

/// Exact credential variable names named by the contracts.
const EXACT_SECRET_NAMES: [&str; 4] = [
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "ACTIONS_RUNTIME_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
];

/// Case-insensitive substrings that mark a name as credential-like.
const SECRET_SUBSTRINGS: [&str; 8] = [
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "CREDENTIAL",
    "PRIVATE_KEY",
    "API_KEY",
    "ACCESS_KEY",
];

/// Whether an environment variable name looks like a credential.
///
/// Matching is case-insensitive; unknown names pass so behavior-affecting
/// variables such as `RUSTFLAGS` keep working.
#[must_use]
pub fn is_secret_env_name(name: &str) -> bool {
    if EXACT_SECRET_NAMES.contains(&name) {
        return true;
    }
    let upper = name.to_ascii_uppercase();
    SECRET_SUBSTRINGS
        .iter()
        .any(|needle| upper.contains(needle))
}
