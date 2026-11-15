// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;
use velnor_model::ToolPins;

use crate::error::CiError;

pub const BUILD_ENV: [(&str, &str); 4] = [
    ("RUSTUP_AUTO_INSTALL", "0"),
    ("CARGO_INCREMENTAL", "0"),
    ("CARGO_TERM_COLOR", "always"),
    ("SOURCE_DATE_EPOCH", "0"),
];

const LINE_LIMIT: usize = 4096;

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

#[derive(Debug, Default)]
pub struct DoctestCount {
    line: Vec<u8>,
    pub ran: u64,
}

impl DoctestCount {
    pub fn feed(&mut self, chunk: &[u8]) {
        for byte in chunk {
            if *byte == b'\n' {
                self.ran += doctests_in(&String::from_utf8_lossy(&self.line));
                self.line.clear();
            } else if self.line.len() < LINE_LIMIT {
                self.line.push(*byte);
            }
        }
    }

    pub fn finish(&mut self) -> u64 {
        self.feed(b"\n");
        self.ran
    }
}

fn doctests_in(line: &str) -> u64 {
    let clean = crate::logscan::strip_ansi(line);
    clean
        .trim()
        .strip_prefix("running ")
        .and_then(|rest| rest.split(' ').next())
        .and_then(|count| count.parse().ok())
        .unwrap_or(0)
}
