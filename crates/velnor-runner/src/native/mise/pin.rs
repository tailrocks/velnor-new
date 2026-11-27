// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ACTION, PREFIX};
use crate::exec::ExecError;

const VERSION_POLICY: &str = include_str!("../../../../../.velnor/version-policy.toml");

fn policy() -> Result<toml::Table, ExecError> {
    VERSION_POLICY.parse().map_err(|error| {
        ExecError::Invalid(format!(
            ".velnor/version-policy.toml is not valid TOML: {error}"
        ))
    })
}

fn pinned_revision() -> Result<String, ExecError> {
    policy()?
        .get("actions")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
        .find(|action| action.get("name").and_then(toml::Value::as_str) == Some(ACTION))
        .and_then(|action| action.get("sha")?.as_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            ExecError::Invalid(format!(
                ".velnor/version-policy.toml pins no revision of {ACTION}"
            ))
        })
}

pub(super) fn require_pinned_revision(reference: &str) -> Result<(), ExecError> {
    let pinned = pinned_revision()?;
    if reference.strip_prefix(PREFIX) == Some(pinned.as_str()) {
        return Ok(());
    }
    Err(ExecError::Unsupported(format!(
        "{reference} is not the {ACTION} revision {pinned} pinned in .velnor/version-policy.toml"
    )))
}

pub(super) fn require_pinned_version(version: &str) -> Result<(), ExecError> {
    let pinned = policy()?
        .get("tools")
        .and_then(|tools| tools.get("mise")?.as_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            ExecError::Invalid(".velnor/version-policy.toml pins no mise version".to_owned())
        })?;
    if version == pinned {
        return Ok(());
    }
    Err(ExecError::Unsupported(format!(
        "mise {version} is not the mise version {pinned} pinned in .velnor/version-policy.toml"
    )))
}
