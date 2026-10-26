// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::is_generated_marker_line;

use crate::evidence_text::{scan_cargo_config, scan_command_text, scan_tool_config};

pub const NEXTEST_RECOMMENDATION: &str = "nextest_recommendation";

pub const PERSIST_EVIDENCE: &str = "persist_evidence";

pub const SHADOWED_NEXTEST_CONFIG: &str = "shadowed_nextest_config";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvidenceStrength {
    Durable,
    Transient,
}

impl EvidenceStrength {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Durable => "durable",
            Self::Transient => "transient",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Evidence {
    pub path: String,
    pub line: u32,
    pub command_or_setting: String,
    pub strength: EvidenceStrength,
}

#[derive(Debug, Clone, Copy)]
pub struct EvidenceFile<'a> {
    pub path: &'a str,
    pub content: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseWrapperInput {
    pub path: String,
    pub line: u32,
    pub command: String,
    pub shim_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextestConfigInput {
    pub path: String,
    pub profiles: Vec<String>,
    pub ci_line: Option<u32>,
}

#[must_use]
pub fn is_generated_output(content: &str) -> bool {
    content.lines().next().is_some_and(is_generated_marker_line)
}

#[must_use]
pub fn evidence_scan_excluded(path: &str) -> bool {
    if is_github_path(path) {
        return true;
    }
    if is_lock_path(path) {
        return true;
    }
    if is_doc_path(path) {
        return true;
    }
    if path == "target" || path.starts_with("target/") || path.contains("/target/") {
        return true;
    }
    path == ".git" || path.starts_with(".git/")
}

fn is_github_path(path: &str) -> bool {
    path == ".github" || path.starts_with(".github/")
}

fn is_doc_path(path: &str) -> bool {
    if path == "docs" || path.starts_with("docs/") {
        return true;
    }
    path.rsplit('/').next().is_some_and(|file| {
        file.starts_with("README")
            || ["md", "mdx", "markdown", "rst", "adoc"]
                .iter()
                .any(|extension| has_extension(file, extension))
    })
}

fn has_extension(file: &str, extension: &str) -> bool {
    std::path::Path::new(file)
        .extension()
        .is_some_and(|found| found.eq_ignore_ascii_case(extension))
}

fn is_lock_path(path: &str) -> bool {
    path.rsplit('/').next().is_some_and(|file| {
        file == "Cargo.lock" || file == "mise.lock" || has_extension(file, "lock")
    })
}

pub(crate) fn strength_for(file: &EvidenceFile<'_>, base: EvidenceStrength) -> EvidenceStrength {
    if is_github_path(file.path) {
        EvidenceStrength::Transient
    } else {
        base
    }
}

pub(crate) fn wrapper_sightings(wrappers: &[MiseWrapperInput]) -> (Vec<Evidence>, Vec<Evidence>) {
    let mut mbx = Vec::new();
    let mut other = Vec::new();
    for input in wrappers {
        let file = EvidenceFile {
            path: &input.path,
            content: "",
        };
        let strength = strength_for(&file, EvidenceStrength::Durable);
        let sighting = Evidence {
            path: input.path.clone(),
            line: input.line,
            command_or_setting: wrapper_text(input),
            strength,
        };
        if input.command == "mbx" {
            mbx.push(sighting);
        } else {
            other.push(sighting);
        }
    }
    (mbx, other)
}

fn wrapper_text(input: &MiseWrapperInput) -> String {
    match &input.shim_mode {
        Some(mode) => format!(
            "wrappers.cargo.command = {:?} (MBX_CARGO_SHIM_MODE = {mode:?})",
            input.command
        ),
        None => format!("wrappers.cargo.command = {:?}", input.command),
    }
}

pub(crate) fn nextest_config_sightings(configs: &[NextestConfigInput]) -> Vec<Evidence> {
    configs
        .iter()
        .map(|input| {
            let file = EvidenceFile {
                path: &input.path,
                content: "",
            };
            Evidence {
                path: input.path.clone(),
                line: input.ci_line.unwrap_or(1),
                command_or_setting: if input.ci_line.is_some() {
                    "[profile.ci]".to_owned()
                } else {
                    "nextest default profile (no [profile.ci])".to_owned()
                },
                strength: strength_for(&file, EvidenceStrength::Durable),
            }
        })
        .collect()
}

#[derive(Debug, Default)]
pub(crate) struct Seen {
    pub(crate) mbx_durable: Vec<Evidence>,
    pub(crate) mbx_transient: Vec<Evidence>,
    pub(crate) nextest_durable: Vec<Evidence>,
    pub(crate) nextest_transient: Vec<Evidence>,
    pub(crate) cargo_durable: Vec<Evidence>,
    pub(crate) cargo_transient: Vec<Evidence>,
    pub(crate) wrapper_other: Vec<Evidence>,
}

impl Seen {
    pub(crate) fn has_durable(&self) -> bool {
        !self.mbx_durable.is_empty()
            || !self.nextest_durable.is_empty()
            || !self.cargo_durable.is_empty()
            || !self.wrapper_other.is_empty()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.mbx_durable.is_empty()
            && self.mbx_transient.is_empty()
            && self.nextest_durable.is_empty()
            && self.nextest_transient.is_empty()
            && self.cargo_durable.is_empty()
            && self.cargo_transient.is_empty()
            && self.wrapper_other.is_empty()
    }

    pub(crate) fn has_runner(&self) -> bool {
        !self.nextest_durable.is_empty()
            || !self.nextest_transient.is_empty()
            || !self.cargo_durable.is_empty()
            || !self.cargo_transient.is_empty()
    }

    pub(crate) fn all_sorted(mut self) -> Vec<Evidence> {
        let mut out = self.mbx_durable;
        out.append(&mut self.mbx_transient);
        out.append(&mut self.nextest_durable);
        out.append(&mut self.nextest_transient);
        out.append(&mut self.cargo_durable);
        out.append(&mut self.cargo_transient);
        out.append(&mut self.wrapper_other);
        out.sort();
        out
    }
}

pub(crate) fn collect_evidence(
    tool_config: Option<&EvidenceFile<'_>>,
    cargo_configs: &[EvidenceFile<'_>],
    executables: &[EvidenceFile<'_>],
    handwritten: &[EvidenceFile<'_>],
    mise_wrappers: &[MiseWrapperInput],
    nextest_configs: &[NextestConfigInput],
) -> Seen {
    let mut seen = Seen::default();
    if let Some(config) = tool_config {
        push_mbx(&mut seen, scan_tool_config(config));
    }
    for config in cargo_configs {
        push_mbx(&mut seen, scan_cargo_config(config));
    }
    for file in executables {
        let (mbx, nextest, cargo) = scan_command_text(file, EvidenceStrength::Durable);
        push_mbx(&mut seen, mbx);
        push_runner(&mut seen, nextest, cargo);
    }
    for file in handwritten {
        let (mbx, nextest, cargo) = scan_command_text(file, EvidenceStrength::Transient);
        push_mbx(&mut seen, mbx);
        push_runner(&mut seen, nextest, cargo);
    }
    let (mbx, other) = wrapper_sightings(mise_wrappers);
    push_mbx(&mut seen, mbx);
    push_other(&mut seen, other);
    push_nextest_configs(&mut seen, nextest_config_sightings(nextest_configs));
    seen
}

fn push_other(seen: &mut Seen, sightings: Vec<Evidence>) {
    seen.wrapper_other.extend(sightings);
}

fn push_nextest_configs(seen: &mut Seen, sightings: Vec<Evidence>) {
    for sighting in sightings {
        match sighting.strength {
            EvidenceStrength::Durable => seen.nextest_durable.push(sighting),
            EvidenceStrength::Transient => seen.nextest_transient.push(sighting),
        }
    }
}

fn push_mbx(seen: &mut Seen, sightings: Vec<Evidence>) {
    for sighting in sightings {
        match sighting.strength {
            EvidenceStrength::Durable => seen.mbx_durable.push(sighting),
            EvidenceStrength::Transient => seen.mbx_transient.push(sighting),
        }
    }
}

fn push_runner(seen: &mut Seen, nextest: Vec<Evidence>, cargo: Vec<Evidence>) {
    for sighting in nextest {
        match sighting.strength {
            EvidenceStrength::Durable => seen.nextest_durable.push(sighting),
            EvidenceStrength::Transient => seen.nextest_transient.push(sighting),
        }
    }
    for sighting in cargo {
        match sighting.strength {
            EvidenceStrength::Durable => seen.cargo_durable.push(sighting),
            EvidenceStrength::Transient => seen.cargo_transient.push(sighting),
        }
    }
}
