// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;
use velnor_model::ToolPins;
use velnor_rust::BuildProfile;

use crate::error::CiError;

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

pub const RUSTUP_AUTO_INSTALL: (&str, &str) = ("RUSTUP_AUTO_INSTALL", "0");

#[must_use]
pub fn env_lines(profile: &BuildProfile) -> String {
    std::iter::once(&RUSTUP_AUTO_INSTALL)
        .chain(&profile.env)
        .map(|(name, value)| format!("{name}={value}\n"))
        .collect::<Vec<_>>()
        .concat()
}

#[must_use]
pub fn output_lines(compat: &str) -> String {
    format!("{COMPAT}={compat}\n")
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
