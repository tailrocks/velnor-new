// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Value;
use velnor_model::{RepoPath, Toolchain};

use crate::error::{DetectError, Reason};
use crate::toml_file::table;

const KEYS: [&str; 5] = ["channel", "path", "components", "targets", "profile"];

pub(crate) const TOOLCHAIN_TOML: RepoPath = RepoPath::from_static("rust-toolchain.toml");
pub(crate) const TOOLCHAIN_PLAIN: RepoPath = RepoPath::from_static("rust-toolchain");

pub(crate) fn toolchain(
    files: &BTreeMap<RepoPath, Vec<u8>>,
) -> Result<Option<Toolchain>, DetectError> {
    [TOOLCHAIN_PLAIN, TOOLCHAIN_TOML]
        .iter()
        .find_map(|path| files.get(path).map(|bytes| (path, bytes)))
        .map(|(path, bytes)| {
            parse(bytes, *path == TOOLCHAIN_PLAIN)
                .map_err(|reason| DetectError::invalid(path, reason))
        })
        .transpose()
}

fn parse(bytes: &[u8], bare_channel_allowed: bool) -> Result<Toolchain, Reason> {
    let text = std::str::from_utf8(bytes).map_err(|_| Reason::NotUtf8)?;
    match (text.lines().count(), bare_channel_allowed) {
        (0, _) => Err(Reason::MissingKey("toolchain.channel")),
        (1, true) => Ok(Toolchain {
            channel: channel(text.trim())?,
            components: Vec::new(),
            targets: Vec::new(),
            profile: None,
        }),
        _ => structured(bytes),
    }
}

fn channel(text: &str) -> Result<String, Reason> {
    if text.is_empty()
        || !text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '+'))
    {
        return Err(Reason::InvalidValue {
            key: "toolchain.channel",
            value: text.to_owned(),
        });
    }
    Ok(text.to_owned())
}

fn structured(bytes: &[u8]) -> Result<Toolchain, Reason> {
    let file = table(bytes)?;
    let section = file
        .get("toolchain")
        .and_then(Value::as_table)
        .ok_or(Reason::MissingTable("toolchain"))?;
    if let Some(key) = section.keys().find(|key| !KEYS.contains(&key.as_str())) {
        return Err(Reason::UnknownKey(key.clone()));
    }
    let name = section
        .get("channel")
        .and_then(Value::as_str)
        .ok_or(Reason::MissingKey("toolchain.channel"))?;
    let profile = section
        .get("profile")
        .map(|value| text(value, "toolchain.profile"))
        .transpose()?;
    Ok(Toolchain {
        channel: channel(name)?,
        components: strings(section, "components", "toolchain.components")?,
        targets: strings(section, "targets", "toolchain.targets")?,
        profile,
    })
}

fn text(value: &Value, key: &'static str) -> Result<String, Reason> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Reason::InvalidValue {
            key,
            value: value.to_string(),
        })
}

fn strings(section: &toml::Table, name: &str, key: &'static str) -> Result<Vec<String>, Reason> {
    match section.get(name) {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items.iter().map(|item| text(item, key)).collect(),
        Some(other) => Err(Reason::InvalidValue {
            key,
            value: other.to_string(),
        }),
    }
}
