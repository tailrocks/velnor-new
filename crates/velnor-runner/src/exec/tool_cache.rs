// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

pub const TOOL_CACHE_VARIABLES: [&str; 3] = [
    "RUNNER_TOOL_CACHE",
    "RUNNER_TOOLSDIRECTORY",
    "AGENT_TOOLSDIRECTORY",
];

#[must_use]
pub fn tool_cache_from_env() -> Option<PathBuf> {
    let value = TOOL_CACHE_VARIABLES.iter().find_map(std::env::var_os)?;
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    Some(std::path::absolute(&path).unwrap_or(path))
}
