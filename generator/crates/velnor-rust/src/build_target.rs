// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView};

use crate::error::{DetectError, Reason};
use crate::target_dir::nearest_first;
use crate::toml_file::table;

const HOST: &str = "host-tuple";
const KEY: &str = "build.target";

struct ConfigTargets {
    named: Vec<(RepoPath, String)>,
    configured: bool,
    host: Option<RepoPath>,
}

pub(crate) fn build_targets(
    view: &RepoView,
    manifest: &RepoPath,
) -> Result<Vec<(RepoPath, String)>, DetectError> {
    config_targets(view, manifest).map(|config| config.named)
}

pub(crate) fn host_tuple_config(
    view: &RepoView,
    manifest: &RepoPath,
) -> Result<Option<RepoPath>, DetectError> {
    config_targets(view, manifest).map(|config| config.host)
}

pub(crate) fn check_host_tuple_version(
    view: &RepoView,
    manifest: &RepoPath,
    version: &str,
) -> Result<(), DetectError> {
    let Some(file) = host_tuple_config(view, manifest)? else {
        return Ok(());
    };
    if !supports_host_tuple(version) {
        return Err(DetectError::invalid(
            &file,
            Reason::HostTupleVersion(version.to_owned()),
        ));
    }
    Ok(())
}

fn supports_host_tuple(version: &str) -> bool {
    let channel = match version.split_once('-') {
        Some((channel @ ("stable" | "beta" | "nightly"), host))
            if host.contains('_') || host.starts_with("aarch64-") =>
        {
            channel
        }
        _ => version,
    };
    match channel {
        "stable" | "beta" | "nightly" => true,
        _ => {
            if let Some(date) = channel.strip_prefix("nightly-") {
                return valid_nightly_date(date) && date >= "2025-09-26";
            }
            release_channel(channel).is_some_and(|(major, minor)| (major, minor) >= (1, 91))
        }
    }
}

fn valid_nightly_date(date: &str) -> bool {
    if date.len() != 10
        || !date.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7) {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
    {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        date[..4].parse::<u16>(),
        date[5..7].parse::<u8>(),
        date[8..].parse::<u8>(),
    ) else {
        return false;
    };
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

fn release_channel(version: &str) -> Option<(u16, u16)> {
    let release = version.split_once('-').map_or(version, |(release, host)| {
        if host.is_empty()
            || !host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return "";
        }
        release
    });
    let parts: Vec<_> = release.split('.').collect();
    if !(2..=3).contains(&parts.len())
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let major = parts[0].parse().ok()?;
    let minor = parts[1].parse().ok()?;
    Some((major, minor))
}

fn config_targets(view: &RepoView, manifest: &RepoPath) -> Result<ConfigTargets, DetectError> {
    let mut named = Vec::new();
    let mut kind = None;
    let mut scalar_seen = false;
    let mut configured = false;
    let mut host = None;
    for (file, bytes, _) in nearest_first(view, manifest) {
        let parsed = table(bytes).map_err(|reason| DetectError::invalid(file, reason))?;
        let Some(value) = parsed.get("build").and_then(|build| build.get("target")) else {
            continue;
        };
        let invalid = || {
            DetectError::invalid(
                file,
                Reason::InvalidValue {
                    key: KEY,
                    value: value.to_string(),
                },
            )
        };
        match value {
            toml::Value::String(one) => {
                configured = true;
                if kind == Some(true) {
                    return Err(invalid());
                }
                kind = Some(false);
                if !scalar_seen {
                    named.push((file.clone(), one.clone()));
                    host = (one == HOST).then(|| file.clone());
                    scalar_seen = true;
                }
            }
            toml::Value::Array(many) => {
                configured |= !many.is_empty();
                if kind == Some(false) {
                    return Err(invalid());
                }
                kind = Some(true);
                for item in many {
                    let name = item.as_str().ok_or_else(invalid)?;
                    if !scalar_seen {
                        named.push((file.clone(), name.to_owned()));
                        if name == HOST {
                            host = Some(file.clone());
                        }
                    }
                }
            }
            _ => return Err(invalid()),
        }
    }
    named.retain(|(_, name)| name != HOST);
    Ok(ConfigTargets {
        named,
        configured,
        host,
    })
}

pub(crate) fn sets_build_target(view: &RepoView, manifest: &RepoPath) -> Result<bool, DetectError> {
    config_targets(view, manifest).map(|config| config.configured)
}
