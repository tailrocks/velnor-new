// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::Deserialize;
use velnor_model::{DenyConfig, RepoPath, RustCiConfig, RustRoot};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDeny {
    locked: Option<bool>,
    config: Option<String>,
}

pub(crate) fn parse(raw: Option<RawDeny>) -> Result<DenyConfig, Reason> {
    let Some(raw) = raw else {
        return Ok(DenyConfig::default());
    };
    let config = raw
        .config
        .map(|file| {
            let invalid = || Reason::InvalidValue {
                key: "deny.config",
                value: file.clone(),
            };
            if file.contains(['\'', '"', '$', '`', '\\']) || file.ends_with('/') {
                return Err(invalid());
            }
            RepoPath::new(file.clone()).map_err(|_| invalid())
        })
        .transpose()?;
    Ok(DenyConfig {
        locked: raw.locked.unwrap_or(false),
        config,
    })
}

pub(crate) fn check(
    config: &RustCiConfig,
    rust_roots: &[RustRoot],
    tracked: &BTreeSet<RepoPath>,
) -> Result<(), DetectError> {
    let Some(file) = &config.deny.config else {
        return Ok(());
    };
    match rust_roots
        .iter()
        .map(|root| root.manifest.sibling(file))
        .find(|path| !tracked.contains(path))
    {
        Some(path) => Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::MissingDenyConfig(path),
        )),
        None => Ok(()),
    }
}
