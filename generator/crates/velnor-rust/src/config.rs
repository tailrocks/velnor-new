// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{PathGlob, RepoPath, RunnerLabel, RustCiConfig};

use crate::error::{DetectError, Reason};
use crate::toml_file::table;

pub const CONFIG_PATH: RepoPath = RepoPath::from_static(".velnor/rust-ci.toml");

const KEYS: [&str; 3] = ["runner", "timeout_minutes", "exclude"];
const TIMEOUT_RANGE: std::ops::RangeInclusive<u16> = 1..=360;

#[derive(Deserialize)]
struct RawConfig {
    runner: Option<String>,
    timeout_minutes: Option<u16>,
    #[serde(default)]
    exclude: Vec<String>,
}

pub(crate) fn parse(bytes: &[u8]) -> Result<RustCiConfig, DetectError> {
    let invalid = |reason| DetectError::invalid(&CONFIG_PATH, reason);
    let table = table(bytes).map_err(invalid)?;
    if let Some(key) = table.keys().find(|key| !KEYS.contains(&key.as_str())) {
        return Err(invalid(Reason::UnknownKey(key.clone())));
    }
    let raw: RawConfig = table
        .try_into()
        .map_err(|error: toml::de::Error| invalid(Reason::Syntax(error.message().to_owned())))?;
    let defaults = RustCiConfig::default();
    let timeout_minutes = raw.timeout_minutes.unwrap_or(defaults.timeout_minutes);
    if !TIMEOUT_RANGE.contains(&timeout_minutes) {
        return Err(invalid(Reason::OutOfRange {
            key: "timeout_minutes",
            value: timeout_minutes,
            min: *TIMEOUT_RANGE.start(),
            max: *TIMEOUT_RANGE.end(),
        }));
    }
    Ok(RustCiConfig {
        runner: match raw.runner {
            Some(label) => RunnerLabel::new(label.clone()).map_err(|_| {
                invalid(Reason::InvalidValue {
                    key: "runner",
                    value: label,
                })
            })?,
            None => defaults.runner,
        },
        timeout_minutes,
        exclude: raw
            .exclude
            .into_iter()
            .map(|entry| {
                PathGlob::new(entry.trim_end_matches('/')).map_err(|_| {
                    invalid(Reason::InvalidValue {
                        key: "exclude",
                        value: entry,
                    })
                })
            })
            .collect::<Result<_, _>>()?,
    })
}

pub(crate) fn excluded(config: &RustCiConfig, manifest: &RepoPath) -> bool {
    config.exclude.iter().any(|entry| entry.matches(manifest))
}
