// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Value;
use velnor_model::RepoPath;

use crate::mise::MISE_CONFIGS;
use crate::toml_file::table;

fn runner_os(runner: &str) -> Option<&'static str> {
    let label = runner.to_ascii_lowercase();
    ["linux", "ubuntu", "macos", "windows"]
        .into_iter()
        .find(|name| label.contains(name))
        .map(|name| if name == "ubuntu" { "linux" } else { name })
}

fn filter_applies(filter: &str, os: &str) -> bool {
    let name = filter.split('/').next().unwrap_or_default();
    let name = if name == "darwin" { "macos" } else { name };
    name == os
}

fn entry_applies(entry: &Value, os: &str) -> bool {
    match entry {
        Value::Array(items) => items.iter().any(|item| entry_applies(item, os)),
        Value::Table(table) => match table.get("os") {
            Some(Value::Array(filters)) => filters
                .iter()
                .filter_map(Value::as_str)
                .any(|filter| filter_applies(filter, os)),
            Some(Value::String(filter)) => filter_applies(filter, os),
            _ => true,
        },
        _ => true,
    }
}

pub(crate) fn pinned_for_runner(
    files: &BTreeMap<RepoPath, Vec<u8>>,
    is_tool: impl Fn(&str) -> bool,
    runner: &str,
) -> bool {
    let os = runner_os(runner);
    MISE_CONFIGS
        .iter()
        .filter_map(|path| table(files.get(path)?).ok())
        .filter_map(|parsed| parsed.get("tools")?.as_table().cloned())
        .flatten()
        .filter(|(name, _)| is_tool(name))
        .any(|(_, entry)| os.is_none_or(|os| entry_applies(&entry, os)))
}
