// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use toml::Table;

use crate::error::ReleaseError;

fn parse(file: &str, text: &str) -> Result<Table, ReleaseError> {
    text.parse::<Table>()
        .map_err(|error| ReleaseError::InvalidToml {
            file: file.to_owned(),
            message: error.to_string(),
        })
}

fn names_release_plz(key: &str) -> bool {
    key.rsplit(['/', ':']).next() == Some("release-plz")
}

fn release_plz_tools(table: &Table) -> Vec<(&String, &toml::Value)> {
    table
        .get("tools")
        .and_then(toml::Value::as_table)
        .map(|tools| {
            tools
                .iter()
                .filter(|(key, _)| names_release_plz(key))
                .collect()
        })
        .unwrap_or_default()
}

fn configured_version(value: &toml::Value) -> Option<&str> {
    match value {
        toml::Value::String(version) => Some(version),
        toml::Value::Table(table) => table.get("version").and_then(toml::Value::as_str),
        _ => None,
    }
}

fn locked_versions(value: &toml::Value) -> Vec<String> {
    let entries = match value {
        toml::Value::Array(entries) => entries.iter().collect(),
        other => vec![other],
    };
    entries
        .into_iter()
        .filter_map(|entry| entry.get("version")?.as_str().map(str::to_owned))
        .collect()
}

pub(crate) fn check(mise_toml: &str, mise_lock: &str) -> Result<(), ReleaseError> {
    let mise_toml = parse("mise.toml", mise_toml)?;
    let mise_lock = parse("mise.lock", mise_lock)?;
    let configured = release_plz_tools(&mise_toml);
    if configured.is_empty() {
        return Err(ReleaseError::ReleasePlzNotInMiseToml);
    }
    let locked = release_plz_tools(&mise_lock);
    let mut mismatch = None;
    for (key, value) in configured {
        let Some((_, entry)) = locked.iter().find(|(locked_key, _)| *locked_key == key) else {
            continue;
        };
        let versions = locked_versions(entry);
        let wanted = configured_version(value).unwrap_or_default();
        if versions.iter().any(|version| version == wanted) {
            return Ok(());
        }
        mismatch = Some(ReleaseError::ReleasePlzLockMismatch {
            configured: wanted.to_owned(),
            locked: versions,
        });
    }
    Err(mismatch.unwrap_or(ReleaseError::ReleasePlzNotInMiseLock))
}
