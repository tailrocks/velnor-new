// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Value;
use velnor_model::{RepoFacts, RepoPath, RustMember, ToolPin, ToolPins};

use crate::error::{DetectError, Reason};
use crate::toml_file::table;

pub(crate) static MISE_CONFIGS: [RepoPath; 2] = [
    RepoPath::from_static("mise.toml"),
    RepoPath::from_static(".mise.toml"),
];
pub(crate) const MISE_LOCK: RepoPath = RepoPath::from_static("mise.lock");
const RUST_TOOLS: [&str; 2] = ["rust", "core:rust"];
const OVERRIDING_ENV: [&str; 2] = ["RUSTDOCFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS"];
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
        let env = table(bytes)
            .map_err(|reason| DetectError::invalid(config, reason))?
            .get("env")
            .and_then(Value::as_table)
            .map(|env| env.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        if let Some(name) = env
            .into_iter()
            .find(|name| OVERRIDING_ENV.contains(&name.as_str()))
        {
            return Err(DetectError::invalid(
                config,
                Reason::MiseOverridesFlags(name),
            ));
        }
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

pub(crate) fn nextest_tool(pins: &ToolPins) -> Option<&str> {
    pins.tools.keys().map(String::as_str).find(|name| {
        NEXTEST_SELECTORS.iter().any(|selector| {
            name.strip_suffix(selector)
                .is_some_and(|rest| rest.is_empty() || rest.ends_with([':', '/']))
        })
    })
}

const MISE_CONFIG: RepoPath = RepoPath::from_static("mise.toml");

pub fn check_tools(facts: &RepoFacts) -> Result<(), DetectError> {
    if let Some((file, mise)) = toolchain_conflict(facts) {
        return Err(DetectError::ToolchainConflict { file, mise });
    }
    let tested = facts
        .rust_roots
        .iter()
        .flat_map(|root| &root.members)
        .any(RustMember::has_test_target);
    if tested && nextest_tool(&facts.tools).is_none() {
        return Err(DetectError::NextestRequired {
            path: facts.tools.configs.first().cloned().unwrap_or(MISE_CONFIG),
        });
    }
    Ok(())
}

fn toolchain_conflict(facts: &RepoFacts) -> Option<(String, String)> {
    let toolchain = facts.toolchain.as_ref()?;
    let channel = toolchain
        .channel
        .clone()
        .or_else(|| toolchain.path.as_ref().map(|path| format!("path {path}")))?;
    let pinned = facts
        .tools
        .tools
        .get(rust_tool(&facts.tools)?)?
        .requested
        .first()?;
    (*pinned != channel).then(|| (channel, pinned.clone()))
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
