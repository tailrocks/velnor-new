// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use velnor_model::{ToolPins, Toolchain};

use crate::error::CiError;

pub const BUILD_ENV: [(&str, &str); 4] = [
    ("RUSTUP_AUTO_INSTALL", "0"),
    ("CARGO_INCREMENTAL", "0"),
    ("CARGO_TERM_COLOR", "always"),
    ("SOURCE_DATE_EPOCH", "0"),
];

pub const COMPAT: &str = "compat";

#[derive(Debug, Deserialize)]
pub struct Installed {
    version: String,
    #[serde(default)]
    installed: bool,
}

pub type Reported = BTreeMap<String, Vec<Installed>>;

pub fn parse_reported(json: &str) -> Result<Reported, CiError> {
    serde_json::from_str(json)
        .map_err(|error| CiError::new(format!("mise ls --json is not readable: {error}")))
}

#[must_use]
pub fn env_lines() -> String {
    BUILD_ENV
        .map(|(name, value)| format!("{name}={value}\n"))
        .concat()
}

#[must_use]
pub fn output_lines() -> String {
    format!("{COMPAT}=\n")
}

pub fn check_tools(pins: &ToolPins, reported: &Reported) -> Result<(), CiError> {
    let wrong: Vec<String> = pins
        .tools
        .iter()
        .filter_map(|(name, pin)| {
            let wanted = if pin.locked.is_empty() {
                &pin.requested
            } else {
                &pin.locked
            };
            let found: Vec<&Installed> = reported
                .get(name)
                .into_iter()
                .flatten()
                .filter(|entry| entry.installed)
                .collect();
            let agrees = found
                .iter()
                .any(|entry| wanted.iter().any(|want| same_version(want, &entry.version)));
            if agrees {
                None
            } else {
                let got = found
                    .first()
                    .map_or("nothing installed", |e| e.version.as_str());
                Some(format!(
                    "{name}: pinned {}, mise reports {got}",
                    wanted.join(" or ")
                ))
            }
        })
        .collect();
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(CiError::new(format!(
            "a tool is not at its pinned version: {}",
            wrong.join("; ")
        )))
    }
}

fn same_version(wanted: &str, installed: &str) -> bool {
    wanted == installed
        || wanted == "latest"
        || installed
            .strip_prefix(wanted)
            .is_some_and(|rest| rest.starts_with('.'))
}

const DEFAULT_PROFILE: [&str; 3] = ["rust-docs", "rustfmt", "clippy"];
const COMPLETE_PROFILE: &str = "complete";
const DEFAULT_PROFILE_NAME: &str = "default";
const TARGETLESS: &str = "rust-src";

pub fn check_toolchain(
    declared: &Toolchain,
    host: &str,
    components: &str,
    targets: &str,
) -> Result<(), CiError> {
    let suffix = format!("-{host}");
    let short = |name: &str| name.strip_suffix(&suffix).unwrap_or(name).to_owned();
    let mut offered = BTreeSet::new();
    let mut installed = BTreeSet::new();
    for line in components.lines() {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else { continue };
        if name == TARGETLESS || name.ends_with(&suffix) {
            offered.insert(short(name));
            if fields.next().is_some() {
                installed.insert(short(name));
            }
        }
    }
    let mut required: BTreeSet<String> = declared.components.iter().map(|c| short(c)).collect();
    match declared.profile.as_deref() {
        Some(DEFAULT_PROFILE_NAME) => required.extend(DEFAULT_PROFILE.map(str::to_owned)),
        Some(COMPLETE_PROFILE) => required.extend(offered),
        _ => {}
    }
    let present: BTreeSet<&str> = targets.lines().map(str::trim).collect();
    let missing: Vec<String> = required
        .iter()
        .filter(|name| !installed.contains(*name))
        .map(|name| format!("component {name}"))
        .chain(
            declared
                .targets
                .iter()
                .filter(|target| !present.contains(target.as_str()))
                .map(|target| format!("target {target}")),
        )
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(CiError::new(format!(
            "the installed toolchain lacks what the toolchain file declares: {}",
            missing.join(", ")
        )))
    }
}
