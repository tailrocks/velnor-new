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

fn release_plz_keys(table: &Table) -> Vec<String> {
    table
        .get("tools")
        .and_then(toml::Value::as_table)
        .map(|tools| {
            tools
                .keys()
                .filter(|key| names_release_plz(key))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn check(mise_toml: &str, mise_lock: &str) -> Result<(), ReleaseError> {
    let configured = release_plz_keys(&parse("mise.toml", mise_toml)?);
    if configured.is_empty() {
        return Err(ReleaseError::ReleasePlzNotInMiseToml);
    }
    let locked = release_plz_keys(&parse("mise.lock", mise_lock)?);
    if !configured.iter().any(|key| locked.contains(key)) {
        return Err(ReleaseError::ReleasePlzNotInMiseLock);
    }
    Ok(())
}
