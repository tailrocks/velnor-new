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

const APPROVED_KEYS: [&str; 5] = [
    "release-plz",
    "aqua:release-plz/release-plz",
    "github:release-plz/release-plz",
    "ubi:release-plz/release-plz",
    "cargo:release-plz",
];

fn approved(key: &str, value: &toml::Value) -> bool {
    APPROVED_KEYS.contains(&key)
        && value
            .as_table()
            .is_none_or(|options| options.keys().all(|option| option == "version"))
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
        .map(|entry| {
            entry
                .get("version")
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

pub(crate) fn check(mise_tomls: &[String], mise_lock: &str) -> Result<String, ReleaseError> {
    let mise_tomls = mise_tomls
        .iter()
        .map(|text| parse("mise.toml", text))
        .collect::<Result<Vec<_>, _>>()?;
    let mise_lock = parse("mise.lock", mise_lock)?;
    let configured: Vec<_> = mise_tomls.iter().flat_map(release_plz_tools).collect();
    if configured.is_empty() {
        return Err(ReleaseError::ReleasePlzNotInMiseToml);
    }
    if let Some((key, _)) = configured.iter().find(|(key, value)| !approved(key, value)) {
        return Err(ReleaseError::ReleasePlzSource {
            key: (*key).clone(),
        });
    }
    let locked = release_plz_tools(&mise_lock);
    let mut mismatch = None;
    for (key, value) in configured {
        let Some((_, entry)) = locked.iter().find(|(locked_key, _)| *locked_key == key) else {
            continue;
        };
        let versions = locked_versions(entry);
        let wanted = configured_version(value).unwrap_or_default();
        if !wanted.is_empty()
            && !versions.is_empty()
            && versions
                .iter()
                .all(|version| !version.is_empty() && version == wanted)
        {
            return Ok((*key).clone());
        }
        mismatch = Some(ReleaseError::ReleasePlzLockMismatch {
            configured: wanted.to_owned(),
            locked: versions,
        });
    }
    Err(mismatch.unwrap_or(ReleaseError::ReleasePlzNotInMiseLock))
}
