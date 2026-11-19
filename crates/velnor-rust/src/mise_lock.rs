// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Value;
use velnor_model::ToolLock;

use crate::error::Reason;
use crate::toml_file::table;

const PLATFORM_PREFIX: &str = "platforms.";
const RUSTUP_TOOLS: [&str; 2] = ["rust", "core:rust"];

pub(crate) fn locked_tools(bytes: &[u8]) -> Result<BTreeMap<String, Vec<ToolLock>>, Reason> {
    let parsed = table(bytes)?;
    match parsed.get("tools") {
        None => Ok(BTreeMap::new()),
        Some(Value::Table(tools)) => tools
            .iter()
            .map(|(name, value)| {
                locked_rows(value)
                    .map(|rows| (name.clone(), rows))
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

fn strings(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::String(text) => Some(vec![text.clone()]),
        Value::Array(items) => items
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => None,
    }
}

fn locked_rows(value: &Value) -> Option<Vec<ToolLock>> {
    match value {
        Value::String(version) => Some(vec![ToolLock {
            version: version.clone(),
            ..ToolLock::default()
        }]),
        Value::Array(items) => items
            .iter()
            .map(locked_rows)
            .collect::<Option<Vec<_>>>()
            .map(|rows| rows.concat()),
        Value::Table(entry) => {
            let version = entry.get("version")?.as_str()?.to_owned();
            let backend = match entry.get("backend") {
                Some(value) => Some(value.as_str()?.to_owned()),
                None => None,
            };
            let specifiers = match entry.get("specifiers") {
                Some(value) => strings(value)?,
                None => Vec::new(),
            };
            let os = match entry.get("os") {
                Some(value) => Some(strings(value)?),
                None => None,
            };
            let options = match entry.get("options") {
                Some(Value::Table(options)) => options
                    .iter()
                    .map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_owned())))
                    .collect::<Option<BTreeMap<_, _>>>()?,
                None => BTreeMap::new(),
                _ => return None,
            };
            let prebuilt = entry
                .iter()
                .filter_map(|(key, value)| {
                    let platform = key.strip_prefix(PLATFORM_PREFIX)?;
                    value
                        .as_table()?
                        .get("url")?
                        .as_str()
                        .map(|_| platform.to_owned())
                })
                .collect();
            Some(vec![ToolLock {
                version,
                backend,
                specifiers,
                os,
                options,
                prebuilt,
            }])
        }
        _ => None,
    }
}

pub(crate) fn is_rustup_tool(name: &str) -> bool {
    RUSTUP_TOOLS.contains(&name)
}
