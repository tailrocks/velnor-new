// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use toml::Value;
use velnor_model::RunnerLabel;

use crate::error::Reason;
use crate::toml_file::table;

const PLATFORM_PREFIX: &str = "platforms.";
const RUSTUP_TOOLS: [&str; 2] = ["rust", "core:rust"];

pub(crate) fn prebuilt_platforms(
    bytes: &[u8],
) -> Result<BTreeMap<String, BTreeSet<String>>, Reason> {
    let parsed = table(bytes)?;
    let Some(Value::Table(tools)) = parsed.get("tools") else {
        return Ok(BTreeMap::new());
    };
    Ok(tools
        .iter()
        .map(|(name, entry)| {
            let entries: Vec<&Value> = match entry {
                Value::Array(items) => items.iter().collect(),
                other => vec![other],
            };
            let platforms = entries
                .into_iter()
                .filter_map(Value::as_table)
                .flat_map(|entry| entry.iter())
                .filter_map(|(key, value)| {
                    let platform = key.strip_prefix(PLATFORM_PREFIX)?;
                    value
                        .as_table()?
                        .get("url")
                        .and_then(Value::as_str)
                        .map(|_| platform.to_owned())
                })
                .collect();
            (name.clone(), platforms)
        })
        .collect())
}

pub(crate) fn is_rustup_tool(name: &str) -> bool {
    RUSTUP_TOOLS.contains(&name)
}

pub(crate) fn lock_platform(runner: &RunnerLabel) -> Option<&'static str> {
    let label = runner.as_str().to_ascii_lowercase();
    let arm = label.contains("arm");
    if label.starts_with("ubuntu") || label.starts_with("linux") {
        Some(if arm { "linux-arm64" } else { "linux-x64" })
    } else if label.starts_with("macos") {
        let intel = label.ends_with("-13") || label.contains("intel") || label.contains("large");
        Some(if intel { "macos-x64" } else { "macos-arm64" })
    } else if label.starts_with("windows") {
        Some("windows-x64")
    } else {
        None
    }
}
