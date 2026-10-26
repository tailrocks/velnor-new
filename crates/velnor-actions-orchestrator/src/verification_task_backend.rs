// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

pub(super) fn is_pinned_tool_backend(key: &str, backend: &str) -> bool {
    key == "rust" && backend == "core:rust"
        || key == "mr-boxington" && backend == "packslip:github.com/jdx/mr-boxington"
        || key == "hk" && backend == "packslip:github.com/jdx/hk"
        || key == "github:boltffi/boltffi" && backend == key
        || backend.starts_with("aqua:") && backend.len() > "aqua:".len()
        || key.starts_with("github:") && key == backend
}

pub(super) fn valid_github_matching_regex(
    key: &str,
    backend: &str,
    options: &BTreeMap<String, String>,
) -> bool {
    key == backend
        && key
            .strip_prefix("github:")
            .is_some_and(velnor_actions_contract::vcs::is_safe_github_repository)
        && options.len() == 1
        && options.get("matching_regex").is_some_and(|value| {
            !value.is_empty()
                && value.len() <= 512
                && !["{{", "{%", "{#"]
                    .iter()
                    .any(|opener| value.contains(*opener))
                && value.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
        })
}
