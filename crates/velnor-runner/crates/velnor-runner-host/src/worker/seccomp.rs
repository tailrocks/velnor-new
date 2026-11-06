// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const RUNNER_PROFILE: &str = include_str!("seccomp/runner-v1.json");

static RUNNER_SECURITY_OPT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub(crate) fn runner_security_opt() -> String {
    RUNNER_SECURITY_OPT
        .get_or_init(|| {
            let profile = serde_json::from_str::<serde_json::Value>(RUNNER_PROFILE)
                .and_then(|profile| serde_json::to_string(&profile))
                .unwrap_or_else(|_| RUNNER_PROFILE.to_owned());
            format!("seccomp={profile}")
        })
        .clone()
}
