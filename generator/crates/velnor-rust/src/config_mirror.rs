// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::Reason;
use serde::Deserialize;
use velnor_model::RunnerLabel;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCratesMirror {
    runner: String,
    registry: String,
    cainfo: String,
}

pub(crate) fn mirror(raw: RawCratesMirror) -> Result<velnor_model::CratesMirror, Reason> {
    let invalid = |key, value| Reason::InvalidValue { key, value };
    let runner = RunnerLabel::new(raw.runner.clone())
        .map_err(|_| invalid("crates_mirror.runner", raw.runner))?;
    if hosted(runner.as_str()) {
        return Err(invalid("crates_mirror.runner", runner.to_string()));
    }
    if !raw.registry.starts_with("sparse+https://")
        || raw.registry.starts_with("sparse+https:///")
        || !raw.registry.ends_with('/')
        || raw
            .registry
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
        || raw
            .registry
            .contains(['\'', '"', '$', '`', '#', '?', '@', '\\'])
        || !raw
            .registry
            .strip_prefix("sparse+")
            .is_some_and(|registry| {
                url::Url::parse(registry)
                    .is_ok_and(|url| url.scheme() == "https" && url.host_str().is_some())
            })
    {
        return Err(invalid("crates_mirror.registry", raw.registry));
    }
    if !raw.cainfo.starts_with('/')
        || raw.cainfo.chars().any(char::is_control)
        || raw.cainfo.contains(['\'', '"', '$', '`'])
    {
        return Err(invalid("crates_mirror.cainfo", raw.cainfo));
    }
    Ok(velnor_model::CratesMirror {
        runner,
        registry: raw.registry,
        cainfo: raw.cainfo,
    })
}

fn hosted(label: &str) -> bool {
    let label = label.to_ascii_lowercase();
    ["ubuntu-", "macos-", "windows-"].iter().any(|prefix| {
        label.strip_prefix(prefix).is_some_and(|version| {
            let version = version.split("-arm").next().unwrap_or(version);
            version == "latest" || version.chars().all(|c| c.is_ascii_digit() || c == '.')
        })
    })
}
