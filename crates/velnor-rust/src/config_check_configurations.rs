// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{CheckConfigurations, RepoPath, RustCiConfig, RustRoot};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCheckConfigurations {
    root: String,
    configurations: Option<String>,
}

pub(crate) fn parse(raw: Vec<RawCheckConfigurations>) -> Result<Vec<CheckConfigurations>, Reason> {
    raw.into_iter().map(entry).collect()
}

fn entry(raw: RawCheckConfigurations) -> Result<CheckConfigurations, Reason> {
    let root = match raw.root.as_str() {
        "." => None,
        directory => Some(RepoPath::new(directory.trim_end_matches('/')).map_err(|_| {
            Reason::InvalidValue {
                key: "check_configurations.root",
                value: raw.root.clone(),
            }
        })?),
    };
    let configurations = raw
        .configurations
        .map(|file| {
            let plain = file.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/')
            });
            match RepoPath::new(&file) {
                Ok(path) if plain => Ok(path),
                _ => Err(Reason::InvalidValue {
                    key: "check_configurations.configurations",
                    value: file,
                }),
            }
        })
        .transpose()?;
    Ok(CheckConfigurations {
        root,
        configurations,
    })
}

pub(crate) fn check(config: &RustCiConfig, rust_roots: &[RustRoot]) -> Result<(), DetectError> {
    match config.check_configurations.iter().find(|entry| {
        !rust_roots
            .iter()
            .any(|root| root.manifest.parent() == entry.root)
    }) {
        Some(entry) => Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::UnknownCheckConfigurationsRoot(
                entry
                    .root
                    .as_ref()
                    .map_or_else(|| ".".to_owned(), ToString::to_string),
            ),
        )),
        None => Ok(()),
    }
}
