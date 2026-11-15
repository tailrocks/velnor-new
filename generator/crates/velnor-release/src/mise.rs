// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Table;

use crate::error::ReleaseError;

fn parse(file: &str, text: &str) -> Result<Table, ReleaseError> {
    text.parse::<Table>()
        .map_err(|error| ReleaseError::InvalidToml {
            file: file.to_owned(),
            message: error.to_string(),
        })
}

const RELEASE_PLZ_KEYS: [&str; 5] = [
    "release-plz",
    "cargo:release-plz",
    "aqua:release-plz/release-plz",
    "github:release-plz/release-plz",
    "ubi:release-plz/release-plz",
];

fn names_release_plz(key: &str) -> bool {
    RELEASE_PLZ_KEYS.contains(&key)
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
        .map(|entry| {
            entry
                .get("version")
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

pub(crate) fn check(
    mise_toml: &str,
    dot_mise_toml: Option<&str>,
    mise_lock: &str,
) -> Result<(), ReleaseError> {
    let mise_toml = parse("mise.toml", mise_toml)?;
    let dot_mise_toml = dot_mise_toml
        .map(|text| parse(".mise.toml", text))
        .transpose()?;
    let mise_lock = parse("mise.lock", mise_lock)?;
    let mut configured = BTreeMap::new();
    for table in std::iter::once(&mise_toml).chain(&dot_mise_toml) {
        configured.extend(release_plz_tools(table));
    }
    if configured.is_empty() {
        return Err(ReleaseError::ReleasePlzNotInMiseToml);
    }
    let locked = release_plz_tools(&mise_lock);
    for (key, value) in configured {
        let Some((_, entry)) = locked.iter().find(|(locked_key, _)| *locked_key == key) else {
            return Err(ReleaseError::ReleasePlzNotInMiseLock);
        };
        let versions = locked_versions(entry);
        let wanted = configured_version(value).unwrap_or_default();
        let matches = !wanted.is_empty()
            && !versions.is_empty()
            && versions
                .iter()
                .all(|version| !version.is_empty() && version == wanted);
        if !matches {
            return Err(ReleaseError::ReleasePlzLockMismatch {
                configured: wanted.to_owned(),
                locked: versions,
            });
        }
    }
    Ok(())
}
