// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::verification_jobs::build_task_jobs::BuildTaskTool;

pub(super) fn toml_option_string(name: &str, value: &str) -> String {
    if name == "matching_regex" && value == super::BOLTFFI_MATCHING_REGEX {
        format!("'{value}'")
    } else if name == "matching_regex" {
        toml::Value::String(value.to_owned()).to_string()
    } else {
        format!("\"{value}\"")
    }
}

pub(crate) fn valid_github_matching_regex(tool: &BuildTaskTool) -> bool {
    tool.key == tool.backend
        && tool
            .key
            .strip_prefix("github:")
            .is_some_and(velnor_actions_contract::vcs::is_safe_github_repository)
        && tool.config_options.len() == 1
        && tool
            .config_options
            .get("matching_regex")
            .is_some_and(|value| {
                !value.is_empty()
                    && value.len() <= 512
                    && !["{{", "{%", "{#"]
                        .iter()
                        .any(|opener| value.contains(*opener))
                    && value.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
            })
}
