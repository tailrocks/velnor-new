// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::tracking::Tracking;

const PASS_THROUGH: [&str; 12] = [
    "PATH",
    "HOME",
    "USER",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LC_COLLATE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
    "TMPDIR",
];

pub(super) fn host_env(tracking: Option<&Tracking>) -> Vec<(String, String)> {
    std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .filter(|(name, _)| PASS_THROUGH.contains(&name.as_str()))
        .chain(tracking.map(Tracking::pair))
        .collect()
}
