// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Value;
use velnor_model::{RepoPath, ToolPin, ToolPins};

use crate::error::{DetectError, Reason};
use crate::toml_file::table;

pub(crate) static MISE_CONFIGS: [RepoPath; 2] = [
    RepoPath::from_static("mise.toml"),
    RepoPath::from_static(".mise.toml"),
];
pub(crate) const MISE_LOCK: RepoPath = RepoPath::from_static("mise.lock");

type Versions = BTreeMap<String, Vec<String>>;

pub(crate) fn tool_pins(files: &BTreeMap<RepoPath, Vec<u8>>) -> Result<ToolPins, DetectError> {
    let (config, bytes) = MISE_CONFIGS
        .iter()
        .find_map(|path| files.get(path).map(|bytes| (path, bytes)))
        .ok_or_else(|| DetectError::MiseRequired {
            path: MISE_CONFIGS[0].clone(),
        })?;
    let requested = versions(bytes).map_err(|reason| DetectError::invalid(config, reason))?;
    let mut locked = files
        .get(&MISE_LOCK)
        .map(|bytes| versions(bytes).map_err(|reason| DetectError::invalid(&MISE_LOCK, reason)))
        .transpose()?
        .unwrap_or_default();
    Ok(ToolPins {
        config: config.clone(),
        tools: requested
            .into_iter()
            .map(|(name, requested)| {
                let locked = locked.remove(&name).unwrap_or_default();
                (name, ToolPin { requested, locked })
            })
            .collect(),
    })
}

fn versions(bytes: &[u8]) -> Result<Versions, Reason> {
    match table(bytes)?.get("tools") {
        None => Ok(Versions::new()),
        Some(Value::Table(tools)) => tools
            .iter()
            .map(|(name, value)| {
                version_list(value)
                    .map(|list| (name.clone(), list))
                    .ok_or_else(|| Reason::InvalidValue {
                        key: "tools",
                        value: name.clone(),
                    })
            })
            .collect(),
        Some(other) => Err(Reason::InvalidValue {
            key: "tools",
            value: other.to_string(),
        }),
    }
}

fn version_list(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::String(version) => Some(vec![version.clone()]),
        Value::Array(items) => items
            .iter()
            .map(version_list)
            .collect::<Option<Vec<_>>>()
            .map(|lists| lists.concat()),
        Value::Table(entry) => entry.get("version").map_or(Some(Vec::new()), version_list),
        _ => None,
    }
}
