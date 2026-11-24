// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use serde_json::Value;

use super::identity::buildx_version;
use crate::error::CiError;

#[derive(Debug, PartialEq, Eq)]
pub struct Lock {
    pub buildx_version: String,
    pub buildx_sha256: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Plugin {
    pub path: PathBuf,
    pub version: Option<String>,
}

fn fail<T>(message: impl Into<String>) -> Result<T, CiError> {
    Err(CiError::new(message))
}

pub fn parse_lock(text: &str) -> Result<Lock, CiError> {
    let table: toml::Table =
        toml::from_str(text).map_err(|error| CiError::new(format!("runner.lock: {error}")))?;
    let field = |name: &str| {
        table
            .get(name)
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| CiError::new(format!("runner.lock: {name} is missing")))
    };
    Ok(Lock {
        buildx_version: field("buildx_version")?,
        buildx_sha256: field("buildx_sha256")?,
    })
}

pub fn banner(output: &str, lock: &Lock) -> Result<String, CiError> {
    let banner = output.trim();
    if banner.is_empty() {
        return fail("Docker Buildx output has no version.");
    }
    if buildx_version(banner) != Some(lock.buildx_version.as_str()) {
        return fail("Docker Buildx version is not the same as the value in .velnor/runner.lock.");
    }
    Ok(banner.to_owned())
}

pub fn plugin(info: &str) -> Result<Plugin, CiError> {
    let Ok(Value::Array(plugins)) = serde_json::from_str::<Value>(info.trim()) else {
        return fail("Docker output has no CLI plug-in identities.");
    };
    let mut buildx = plugins
        .iter()
        .filter(|plugin| plugin.get("Name").and_then(Value::as_str) == Some("buildx"));
    let (Some(plugin), None) = (buildx.next(), buildx.next()) else {
        return fail("Docker output does not list one Buildx plug-in path.");
    };
    let Some(path) = plugin.get("Path").and_then(Value::as_str) else {
        return fail("Docker output does not list one Buildx plug-in path.");
    };
    Ok(Plugin {
        path: PathBuf::from(path),
        version: plugin
            .get("Version")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}
