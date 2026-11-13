// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{RepoPath, RunnerLabel, RustCiConfig};

use crate::DetectError;

pub const CONFIG_PATH: RepoPath = RepoPath::from_static(".velnor/rust-ci.toml");

const TIMEOUT_RANGE: std::ops::RangeInclusive<u16> = 1..=360;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    runner: Option<String>,
    timeout_minutes: Option<u16>,
    #[serde(default)]
    exclude: Vec<String>,
}

pub(crate) fn parse(bytes: &[u8]) -> Result<RustCiConfig, DetectError> {
    let text = std::str::from_utf8(bytes).map_err(|error| config_error(&error))?;
    let raw: RawConfig = toml::from_str(text).map_err(|error| config_error(&error))?;
    let defaults = RustCiConfig::default();
    let timeout_minutes = raw.timeout_minutes.unwrap_or(defaults.timeout_minutes);
    if !TIMEOUT_RANGE.contains(&timeout_minutes) {
        return Err(DetectError::Config(format!(
            "timeout_minutes must be between {} and {}, got {timeout_minutes}",
            TIMEOUT_RANGE.start(),
            TIMEOUT_RANGE.end()
        )));
    }
    Ok(RustCiConfig {
        runner: raw
            .runner
            .map(RunnerLabel::new)
            .transpose()
            .map_err(|error| config_error(&error))?
            .unwrap_or(defaults.runner),
        timeout_minutes,
        exclude: raw
            .exclude
            .into_iter()
            .map(|path| RepoPath::new(path.trim_end_matches('/')))
            .collect::<Result<_, _>>()
            .map_err(|error| config_error(&error))?,
    })
}

fn config_error(error: &impl std::fmt::Display) -> DetectError {
    DetectError::Config(error.to_string())
}
