// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use velnor_model::{ToolPins, Toolchain};
use velnor_rust::BuildProfile;

use crate::error::CiError;

const LINE_LIMIT: usize = 4096;
pub const COMPAT: &str = "compat";

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
        same_version(wanted, &self.version) || self.requested_version.as_deref() == Some(wanted)
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
                .filter(|entry| entry.present)
                .collect();
            let agrees = found.iter().any(|entry| {
                wanted
                    .iter()
                    .any(|want| entry.satisfies(want, !pin.locked.is_empty()))
            });
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

const MINIMAL_PROFILE: [&str; 3] = ["cargo", "rust-std", "rustc"];
const DEFAULT_PROFILE: [&str; 3] = ["rust-docs", "rustfmt", "clippy"];
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
            required.extend(MINIMAL_PROFILE.map(str::to_owned));
            required.extend(DEFAULT_PROFILE.map(str::to_owned));
        }
        Some(MINIMAL_PROFILE_NAME) => required.extend(MINIMAL_PROFILE.map(str::to_owned)),
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
