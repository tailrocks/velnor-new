// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{CheckConfigurations, RepoPath, RepoView, RustCiConfig, RustRoot};

use crate::config::{CONFIG_PATH, rule_matches};
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

pub(crate) fn check(
    view: &RepoView,
    config: &RustCiConfig,
    rust_roots: &[RustRoot],
) -> Result<(), DetectError> {
    let invalid = |reason| DetectError::invalid(&CONFIG_PATH, reason);
    if let Some(entry) = config.check_configurations.iter().find(|entry| {
        !rust_roots
            .iter()
            .any(|root| root.manifest.parent() == entry.root)
    }) {
        return Err(invalid(Reason::UnknownCheckConfigurationsRoot(
            entry
                .root
                .as_ref()
                .map_or_else(|| ".".to_owned(), ToString::to_string),
        )));
    }
    for file in config
        .check_configurations
        .iter()
        .filter_map(CheckConfigurations::file)
    {
        let linked = std::iter::successors(Some(file.clone()), RepoPath::parent)
            .any(|part| view.symlinks.contains(&part));
        let reason = if !view.tracked_paths.contains(&file) {
            Some("a file that is not tracked in the repository")
        } else if linked {
            Some("a symbolic link, or inside one; name the regular file")
        } else if config
            .non_inputs
            .iter()
            .any(|glob| rule_matches(glob, &file))
        {
            Some("a file that a non_inputs rule ignores")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(invalid(Reason::InvalidValue {
                key: "check_configurations.configurations",
                value: format!("{file}, {reason}"),
            }));
        }
    }
    Ok(())
}
