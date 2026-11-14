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
const RUST_TOOLS: [&str; 2] = ["rust", "core:rust"];
const NEXTEST_SELECTORS: [&str; 2] = ["cargo-nextest", "nextest-rs/nextest"];

type Versions = BTreeMap<String, Vec<String>>;

pub(crate) fn tool_pins(files: &BTreeMap<RepoPath, Vec<u8>>) -> Result<ToolPins, DetectError> {
    let present: Vec<(&RepoPath, &Vec<u8>)> = MISE_CONFIGS
        .iter()
        .filter_map(|path| files.get(path).map(|bytes| (path, bytes)))
        .collect();
    if present.is_empty() {
        return Err(DetectError::MiseRequired {
            path: MISE_CONFIGS[0].clone(),
        });
    }
    let mut requested = Versions::new();
    for (config, bytes) in &present {
        requested.extend(versions(bytes).map_err(|reason| DetectError::invalid(config, reason))?);
    }
    let mut locked = files
        .get(&MISE_LOCK)
        .map(|bytes| versions(bytes).map_err(|reason| DetectError::invalid(&MISE_LOCK, reason)))
        .transpose()?
        .unwrap_or_default();
    Ok(ToolPins {
        configs: present
            .iter()
            .map(|(config, _)| (*config).clone())
            .collect(),
        tools: requested
            .into_iter()
            .map(|(name, requested)| {
                let locked = locked.remove(&name).unwrap_or_default();
                (name, ToolPin { requested, locked })
            })
            .collect(),
    })
}

#[must_use]
pub fn nextest_tool(pins: &ToolPins) -> Option<&str> {
    pins.tools.keys().map(String::as_str).find(|name| {
        NEXTEST_SELECTORS.iter().any(|selector| {
            name.strip_suffix(selector)
                .is_some_and(|rest| rest.is_empty() || rest.ends_with([':', '/']))
        })
    })
}

pub(crate) fn rust_tool(pins: &ToolPins) -> Option<&str> {
    RUST_TOOLS
        .into_iter()
        .find(|name| pins.tools.contains_key(*name))
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
        Value::Table(entry) => entry
            .get("version")
            .map_or_else(|| source_request(entry), version_list),
        _ => None,
    }
}

fn source_request(entry: &toml::Table) -> Option<Vec<String>> {
    ["ref", "path", "prefix"].iter().find_map(|key| {
        entry
            .get(*key)
            .and_then(Value::as_str)
            .map(|value| vec![format!("{key}:{value}")])
    })
}
