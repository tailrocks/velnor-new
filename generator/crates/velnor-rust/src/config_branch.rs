// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::Reason;

pub(crate) fn default_branch(
    configured: Option<String>,
    default: String,
) -> Result<String, Reason> {
    match configured {
        None => Ok(default),
        Some(name) if branch_name(&name) => Ok(name),
        Some(name) => Err(Reason::InvalidValue {
            key: "default_branch",
            value: name,
        }),
    }
}

fn branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['/', '-'])
        && !name.ends_with(['/', '.'])
        && !name.contains("..")
        && !name.contains("//")
        && !name.contains("@{")
        && !name.contains("{{")
        && !name.contains("}}")
        && name != "HEAD"
        && !name.split('/').any(|part| {
            part.starts_with('.') || part.rsplit_once('.').is_some_and(|(_, ext)| ext == "lock")
        })
        && !name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
}
