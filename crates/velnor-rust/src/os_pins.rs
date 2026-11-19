// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Value;
use velnor_model::{MisePlatform, RepoPath};

use crate::mise::MISE_CONFIGS;
use crate::toml_file::table;

fn entry_applies(entry: &Value, platform: MisePlatform<'_>) -> bool {
    match entry {
        Value::Array(items) => items.iter().any(|item| entry_applies(item, platform)),
        Value::Table(table) => match table.get("os") {
            Some(Value::Array(filters)) => filters
                .iter()
                .filter_map(Value::as_str)
                .any(|filter| platform.matches(filter)),
            Some(Value::String(filter)) => platform.matches(filter),
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
    let platform = MisePlatform::runner(runner);
    MISE_CONFIGS
        .iter()
        .filter_map(|path| table(files.get(path)?).ok())
        .filter_map(|parsed| parsed.get("tools")?.as_table().cloned())
        .flatten()
        .filter(|(name, _)| is_tool(name))
        .any(|(_, entry)| platform.is_none_or(|platform| entry_applies(&entry, platform)))
}
