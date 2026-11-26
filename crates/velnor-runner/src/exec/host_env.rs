// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const PASS_THROUGH: [&str; 6] = ["PATH", "HOME", "USER", "LANG", "LC_*", "TMPDIR"];

fn passes(name: &str) -> bool {
    PASS_THROUGH
        .iter()
        .any(|pattern| match pattern.strip_suffix('*') {
            Some(prefix) => name.starts_with(prefix),
            None => *pattern == name,
        })
}

pub(super) fn host_env() -> Vec<(String, String)> {
    std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .filter(|(name, _)| passes(name))
        .collect()
}
