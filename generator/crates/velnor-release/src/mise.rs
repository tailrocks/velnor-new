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

fn has_tool(table: &Table) -> bool {
    table
        .get("tools")
        .and_then(toml::Value::as_table)
        .is_some_and(|tools| tools.keys().any(|key| names_release_plz(key)))
}

pub(crate) fn check(mise_toml: &str, mise_lock: &str) -> Result<(), ReleaseError> {
    if !has_tool(&parse("mise.toml", mise_toml)?) {
        return Err(ReleaseError::ReleasePlzNotInMiseToml);
    }
    if !has_tool(&parse("mise.lock", mise_lock)?) {
        return Err(ReleaseError::ReleasePlzNotInMiseLock);
    }
    Ok(())
}
