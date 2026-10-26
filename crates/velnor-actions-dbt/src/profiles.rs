// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::DbtError;

const SECRET_KEYS: [&str; 7] = [
    "password",
    "token",
    "private_key",
    "private_key_path",
    "private_key_passphrase",
    "keyfile",
    "client_secret",
];

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

#[must_use]
pub fn uses_postgres_adapter(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.split(" #").next().unwrap_or("").trim();
        line.split_once(':').is_some_and(|(key, value)| {
            key.trim()
                .trim_start_matches("- ")
                .trim_matches(['"', '\''])
                == "type"
                && value.trim().trim_matches(['"', '\'']) == "postgres"
        })
    })
}
