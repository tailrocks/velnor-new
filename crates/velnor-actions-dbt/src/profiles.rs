//! Connection-profile hygiene: no literal credential in a checked-in profile.

use crate::DbtError;

/// Profile keys whose value is a credential.
const SECRET_KEYS: [&str; 7] = [
    "password",
    "token",
    "private_key",
    "private_key_path",
    "private_key_passphrase",
    "keyfile",
    "client_secret",
];

/// Findings for `profiles.yml` text: a credential key must read `env_var(...)`.
#[must_use]
pub fn check_profiles(text: &str) -> Vec<DbtError> {
    let mut findings = Vec::new();
    for line in text.lines() {
        let line = line.split(" #").next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key
            .trim()
            .trim_start_matches("- ")
            .trim_matches(['"', '\'']);
        let value = value.trim();
        if SECRET_KEYS.contains(&key) && !value.is_empty() && !value.contains("env_var(") {
            findings.push(DbtError::new("dbt_profile_literal_secret", key));
        }
    }
    findings.sort();
    findings.dedup();
    findings
}
