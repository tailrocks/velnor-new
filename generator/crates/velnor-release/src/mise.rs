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

const RELEASE_PLZ_DOWNLOAD: &str = "https://github.com/release-plz/release-plz/releases/download/";
const RELEASE_PLZ_ASSET: &str =
    "https://api.github.com/repos/release-plz/release-plz/releases/assets/";

fn plain_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

fn own_download(url: &str, wanted: &str) -> bool {
    if let Some(rest) = url.strip_prefix(RELEASE_PLZ_DOWNLOAD) {
        let mut segments = rest.split('/');
        return matches!(
            (segments.next(), segments.next(), segments.next()),
            (Some(tag), Some(asset), None)
                if tag == format!("release-plz-v{wanted}") && plain_segment(asset)
        );
    }
    url.strip_prefix(RELEASE_PLZ_ASSET)
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
}

fn foreign_artifact(value: &toml::Value, wanted: &str) -> Option<String> {
    match value {
        toml::Value::Array(items) => items.iter().find_map(|item| foreign_artifact(item, wanted)),
        toml::Value::Table(table) => table.iter().find_map(|(key, inner)| match inner {
            toml::Value::String(url) if matches!(key.as_str(), "url" | "url_api") => {
                (!own_download(url, wanted)).then(|| url.clone())
            }
            other => foreign_artifact(other, wanted),
        }),
        _ => None,
    }
}

fn redirecting_key(table: &Table) -> Option<&'static str> {
    if table.contains_key("plugins") {
        return Some("plugins");
    }
    let settings = table.get("settings").and_then(toml::Value::as_table)?;
    ["url_replacements", "registry_url", "aqua"]
        .into_iter()
        .find(|key| settings.contains_key(*key))
}

fn exact_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

const MINIMUM_VERSION: [u64; 3] = [0, 3, 168];

fn at_least_minimum(version: &str) -> bool {
    let parts: Vec<u64> = version
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect();
    parts.as_slice() >= MINIMUM_VERSION.as_slice()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReleasePlzDownload {
    pub(crate) version: String,
    pub(crate) url: String,
    pub(crate) sha256: String,
}

fn linux_x64_download(entry: &toml::Value, wanted: &str) -> Option<ReleasePlzDownload> {
    let entries = match entry {
        toml::Value::Array(entries) => entries.iter().collect(),
        other => vec![other],
    };
    let platform = entries
        .into_iter()
        .find(|entry| entry.get("version").and_then(toml::Value::as_str) == Some(wanted))?
        .get("platforms.linux-x64")?;
    let sha256 = platform
        .get("checksum")?
        .as_str()?
        .strip_prefix("sha256:")?
        .to_owned();
    let url = platform.get("url")?.as_str()?.to_owned();
    let hex = sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit());
    (hex && url.starts_with(RELEASE_PLZ_DOWNLOAD) && own_download(&url, wanted)).then(|| {
        ReleasePlzDownload {
            version: wanted.to_owned(),
            url,
            sha256: sha256.to_ascii_lowercase(),
        }
    })
}

pub(crate) fn check(
    mise_tomls: &[String],
    mise_lock: &str,
) -> Result<ReleasePlzDownload, ReleaseError> {
    let mise_tomls = mise_tomls
        .iter()
        .map(|text| parse("mise.toml", text))
        .collect::<Result<Vec<_>, _>>()?;
    let mise_lock = parse("mise.lock", mise_lock)?;
    if let Some(key) = mise_tomls.iter().find_map(redirecting_key) {
        return Err(ReleaseError::MiseRedirectsInstall {
            file: "mise.toml".to_owned(),
            key: key.to_owned(),
        });
    }
    let mut configured = BTreeMap::new();
    for table in &mise_tomls {
        configured.extend(release_plz_tools(table));
    }
    if configured.is_empty() {
        return Err(ReleaseError::ReleasePlzNotInMiseToml);
    }
    let locked = release_plz_tools(&mise_lock);
    let mut pinned: Option<ReleasePlzDownload> = None;
    for (key, value) in configured {
        if let Some(field) = value
            .as_table()
            .and_then(|table| table.keys().find(|field| *field != "version"))
        {
            return Err(ReleaseError::ReleasePlzToolOverride {
                tool: key.clone(),
                field: field.clone(),
            });
        }
        if key == "cargo:release-plz" || key == "ubi:release-plz/release-plz" {
            return Err(ReleaseError::ReleasePlzBackendNotDownloadable { tool: key.clone() });
        }
        let wanted = configured_version(value).unwrap_or_default();
        if !exact_version(wanted) {
            return Err(ReleaseError::ReleasePlzVersionNotExact {
                version: wanted.to_owned(),
            });
        }
        if !at_least_minimum(wanted) {
            return Err(ReleaseError::ReleasePlzVersionTooOld {
                version: wanted.to_owned(),
                minimum: "0.3.168".to_owned(),
            });
        }
        let Some((_, entry)) = locked.iter().find(|(locked_key, _)| *locked_key == key) else {
            return Err(ReleaseError::ReleasePlzNotInMiseLock);
        };
        let versions = locked_versions(entry);
        let matches = !versions.is_empty()
            && versions
                .iter()
                .all(|version| !version.is_empty() && version == wanted);
        if !matches {
            return Err(ReleaseError::ReleasePlzLockMismatch {
                configured: wanted.to_owned(),
                locked: versions,
            });
        }
        if let Some(url) = foreign_artifact(entry, wanted) {
            return Err(ReleaseError::ReleasePlzArtifactElsewhere {
                tool: key.clone(),
                url,
            });
        }
        let download = linux_x64_download(entry, wanted)
            .ok_or_else(|| ReleaseError::ReleasePlzNotLockedForLinux { tool: key.clone() })?;
        if let Some(first) = &pinned
            && first.version != download.version
        {
            return Err(ReleaseError::ReleasePlzVersionsConflict {
                first: first.version.clone(),
                second: download.version,
            });
        }
        pinned = Some(download);
    }
    pinned.ok_or(ReleaseError::ReleasePlzNotInMiseToml)
}
