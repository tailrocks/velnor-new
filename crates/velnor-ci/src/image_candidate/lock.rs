// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::identity::buildx_version;
use crate::error::CiError;

#[derive(Debug, PartialEq, Eq)]
pub struct Lock {
    pub buildx_version: String,
    pub buildx_sha256: Option<String>,
}

fn fail<T>(message: impl Into<String>) -> Result<T, CiError> {
    Err(CiError::new(message))
}

pub fn parse_lock(text: &str) -> Result<Lock, CiError> {
    let table: toml::Table =
        toml::from_str(text).map_err(|error| CiError::new(format!("runner.lock: {error}")))?;
    let field = |name: &str| table.get(name).and_then(toml::Value::as_str);
    let Some(version) = field("buildx_version") else {
        return fail("runner.lock: buildx_version is missing");
    };
    Ok(Lock {
        buildx_version: version.to_owned(),
        buildx_sha256: field("buildx_sha256").map(str::to_owned),
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
