// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use velnor_model::{MisePlatform, ToolPin, ToolPins, Toolchain};
use velnor_rust::BuildProfile;

use crate::error::CiError;

const LINE_LIMIT: usize = 4096;
pub const COMPAT: &str = "compat";
pub const COLD: &str = "cold";

#[derive(Debug, Deserialize)]
pub struct Installed {
    version: String,
    #[serde(default)]
    requested_version: Option<String>,
    #[serde(default, rename = "installed")]
    present: bool,
}

impl Installed {
    fn satisfies(&self, wanted: &str, locked: bool) -> bool {
        if locked {
            return self.version == wanted;
        }
        match self.requested_version.as_deref() {
            Some(request) => request == wanted,
            None => same_version(wanted, &self.version),
        }
    }
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

/// `cold` is set when the run could not read its image identity: its key matches no other run,
/// so a workflow can skip the save that no run could ever restore.
#[must_use]
pub fn output_lines(compat: &str, cold: bool) -> String {
    let cold = if cold {
        format!("{COLD}=true\n")
    } else {
        String::new()
    };
    format!("{COMPAT}={compat}\n{cold}")
}

fn required(pin: &ToolPin, platform: MisePlatform<'_>) -> Result<Vec<String>, CiError> {
    if pin.locked.is_empty() {
        return Ok(pin
            .requested
            .iter()
            .filter(|request| request.applies_on(platform))
            .map(|request| request.version.clone())
            .collect());
    }
    pin.locked_on(platform)
        .map(|rows| rows.into_iter().map(|row| row.version.clone()).collect())
        .map_err(CiError::new)
}

pub fn check_tools(
    pins: &ToolPins,
    reported: &Reported,
    platform: MisePlatform<'_>,
) -> Result<(), CiError> {
    let wrong: Vec<String> = pins
        .tools
        .iter()
        .map(|(name, pin)| {
            let wanted = &required(pin, platform)?;
            let found: Vec<&Installed> = reported
                .get(name)
                .into_iter()
                .flatten()
                .filter(|entry| entry.present)
                .collect();
            let locked = !pin.locked.is_empty();
            let missing: Vec<&str> = wanted
                .iter()
                .filter(|want| !found.iter().any(|entry| entry.satisfies(want, locked)))
                .map(String::as_str)
                .collect();
            if missing.is_empty() {
                Ok(None)
            } else {
                let got = found
                    .iter()
                    .map(|entry| entry.version.as_str())
                    .collect::<Vec<_>>();
                let got = if got.is_empty() {
                    "nothing installed".to_owned()
                } else {
                    got.join(", ")
                };
                Ok(Some(format!(
                    "{name}: pinned {}, mise reports {got}",
                    missing.join(" and ")
                )))
            }
        })
        .collect::<Result<Vec<_>, CiError>>()?
        .into_iter()
        .flatten()
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

const MINIMAL_PROFILE: &[&str] = &["cargo", "rust-std", "rustc"];
const DEFAULT_PROFILE: &[&str] = &["rust-docs", "rustfmt", "clippy"];
const COMPLETE_PROFILE: &str = "complete";
const DEFAULT_PROFILE_NAME: &str = "default";
const MINIMAL_PROFILE_NAME: &str = "minimal";
const PREVIEW: &str = "-preview";
const TARGETLESS: &str = "rust-src";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Missing {
    pub components: Vec<String>,
    pub targets: Vec<String>,
}

impl Missing {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.components.is_empty() && self.targets.is_empty()
    }

    pub fn ensure_none(&self) -> Result<(), CiError> {
        if self.is_empty() {
            return Ok(());
        }
        let names: Vec<String> = self
            .components
            .iter()
            .map(|name| format!("component {name}"))
            .chain(self.targets.iter().map(|name| format!("target {name}")))
            .collect();
        Err(CiError::new(format!(
            "the installed toolchain lacks what the toolchain file declares: {}",
            names.join(", ")
        )))
    }
}

#[must_use]
pub fn missing_toolchain(
    declared: &Toolchain,
    host: &str,
    components: &str,
    targets: &str,
) -> Missing {
    let suffix = format!("-{host}");
    let short = |name: &str| {
        let bare = name.strip_suffix(&suffix).unwrap_or(name);
        bare.strip_suffix(PREVIEW).unwrap_or(bare).to_owned()
    };
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
        Some(DEFAULT_PROFILE_NAME) => {
            required.extend(MINIMAL_PROFILE.iter().copied().map(str::to_owned));
            required.extend(DEFAULT_PROFILE.iter().copied().map(str::to_owned));
        }
        Some(MINIMAL_PROFILE_NAME) => {
            required.extend(MINIMAL_PROFILE.iter().copied().map(str::to_owned));
        }
        Some(COMPLETE_PROFILE) => required.extend(offered),
        _ => {}
    }
    let present: BTreeSet<&str> = targets.lines().map(str::trim).collect();
    Missing {
        components: required
            .into_iter()
            .filter(|name| !installed.contains(name))
            .collect(),
        targets: declared
            .targets
            .iter()
            .filter(|target| !present.contains(target.as_str()))
            .cloned()
            .collect(),
    }
}

#[derive(Debug, Default)]
pub struct DoctestCount {
    line: Vec<u8>,
    confirmed: u64,
    ignored: u64,
}

impl DoctestCount {
    pub fn feed(&mut self, chunk: &[u8]) {
        for byte in chunk {
            if *byte == b'\n' {
                self.end_line();
            } else if self.line.len() < LINE_LIMIT {
                self.line.push(*byte);
            }
        }
    }

    pub fn end_stream(&mut self) {
        self.end_line();
    }

    #[must_use]
    pub const fn ran(&self) -> u64 {
        self.confirmed.saturating_sub(self.ignored)
    }

    fn end_line(&mut self) {
        let line = String::from_utf8_lossy(&self.line);
        if let Some((confirmed, ignored)) = result_counts(&line) {
            self.confirmed += confirmed;
            self.ignored += ignored;
        }
        self.line.clear();
    }
}

fn result_counts(line: &str) -> Option<(u64, u64)> {
    let clean = crate::logscan::strip_ansi(line);
    let rest = clean.trim().strip_prefix("test result:")?;
    let mut passed = None;
    let mut failed = None;
    let mut ignored = None;
    for part in rest.split(';') {
        let mut words = part.split_whitespace().rev();
        let Some(field) = words.next() else {
            continue;
        };
        let Some(value) = words.next() else {
            continue;
        };
        let slot = match field {
            "passed" => &mut passed,
            "failed" => &mut failed,
            "ignored" => &mut ignored,
            _ => continue,
        };
        *slot = Some(value.parse::<u64>().ok()?);
    }
    let passed = passed?;
    let failed = failed?;
    let ignored = ignored?;
    Some((passed + failed + ignored, ignored))
}
