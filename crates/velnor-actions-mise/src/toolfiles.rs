// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "toolfiles_config_tools.rs"]
mod config_tools;
#[path = "mise_lockfile.rs"]
pub mod lockfile;

use std::collections::BTreeMap;
use std::fmt;

use crate::catalog::{PinnedTool, validate_exact_version};
use crate::wrappers::parse_cargo_wrapper;

pub const MISE_TOML_FILE: &str = "mise.toml";
pub const DOT_MISE_TOML_FILE: &str = ".mise.toml";
pub const MISE_LOCK_FILE: &str = "mise.lock";

pub const OWNED_SYMBOLS: &[&str] = &["mise.toml", ".mise.toml", "mise.lock", ".mise.lock"];

pub const FOREIGN_TOOL_FILES: &[&str] = &["rust-toolchain.toml", "Cargo.toml", "Cargo.lock"];

pub const MISE_ENV_PREFIX: &str = "MISE_";

pub const MISSING_RECOMMENDED_INPUT: &str = "missing_recommended_input";

pub const TOOLING_INPUT_INVALID: &str = "tooling_input_invalid";

#[derive(Debug, Clone, Copy)]
pub struct ToolFile<'a> {
    pub path: &'a str,
    pub content: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolFinding {
    pub code: String,
    pub file: String,
    pub observed: Option<String>,
    pub recommendation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MiseSpec {
    pub tools: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseInspection {
    pub file: String,
    pub spec: Option<MiseSpec>,
    pub findings: Vec<ToolFinding>,
}

impl MiseInspection {
    fn missing(path: &str) -> Self {
        Self {
            file: path.to_owned(),
            spec: None,
            findings: vec![ToolFinding {
                code: MISSING_RECOMMENDED_INPUT.to_owned(),
                file: path.to_owned(),
                observed: None,
                recommendation: format!(
                    "add a user-authored [tools] table in a new {path}; Velnor never creates it"
                ),
            }],
        }
    }

    fn invalid(path: &str, problem: &str) -> Self {
        Self {
            file: path.to_owned(),
            spec: None,
            findings: vec![ToolFinding {
                code: TOOLING_INPUT_INVALID.to_owned(),
                file: path.to_owned(),
                observed: Some(problem.to_owned()),
                recommendation: format!(
                    "fix {problem} manually in {path}; generation continues with Velnor pins"
                ),
            }],
        }
    }

    fn present(path: &str, spec: MiseSpec) -> Self {
        Self {
            file: path.to_owned(),
            spec: Some(spec),
            findings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolInspectError {
    NotOwned { path: String },
}

impl fmt::Display for ToolInspectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::NotOwned { path } = self;
        write!(f, "tool_file_not_owned:{path}")
    }
}

impl std::error::Error for ToolInspectError {}

#[must_use]
pub fn stack_for_symbol(path: &str) -> Option<&'static str> {
    let name = file_name(path);
    if OWNED_SYMBOLS.contains(&name) {
        Some(crate::TOOL_ID)
    } else if FOREIGN_TOOL_FILES.contains(&name) {
        Some("rust")
    } else {
        None
    }
}

#[must_use]
pub fn is_owned_mise_file(path: &str) -> bool {
    OWNED_SYMBOLS.contains(&file_name(path))
}

#[must_use]
pub fn is_mise_env_symbol(name: &str) -> bool {
    name.starts_with(MISE_ENV_PREFIX)
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn inspect_mise_file(
    path: &str,
    content: Option<&str>,
) -> Result<MiseInspection, ToolInspectError> {
    if !is_owned_mise_file(path) {
        return Err(ToolInspectError::NotOwned {
            path: path.to_owned(),
        });
    }
    let Some(text) = content else {
        return Ok(MiseInspection::missing(path));
    };
    match parse_tools(path, text) {
        Ok(spec) => Ok(MiseInspection::present(path, spec)),
        Err(problem) => Ok(MiseInspection::invalid(path, &problem)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MbxWrapperStatus {
    NoMbxWrapper,
    Pinned { version: String },
    MissingPin,
    Invalid { problem: String },
}

#[must_use]
pub fn mbx_wrapper_pin_status(content: &str) -> MbxWrapperStatus {
    let wrapper = match parse_cargo_wrapper(content) {
        Ok(wrapper) => wrapper,
        Err(diagnostic) => {
            return MbxWrapperStatus::Invalid {
                problem: diagnostic.to_string(),
            };
        }
    };
    if !wrapper.is_some_and(|found| found.is_mbx()) && !rust_option_selects_mbx(content) {
        return MbxWrapperStatus::NoMbxWrapper;
    }
    let inspection = match inspect_mise_file(MISE_TOML_FILE, Some(content)) {
        Ok(inspection) => inspection,
        Err(error) => {
            return MbxWrapperStatus::Invalid {
                problem: error.to_string(),
            };
        }
    };
    let Some(spec) = inspection.spec else {
        let problem = inspection
            .findings
            .first()
            .and_then(|finding| finding.observed.clone())
            .unwrap_or_else(|| "unreadable_tools".to_owned());
        return MbxWrapperStatus::Invalid { problem };
    };
    let tool = PinnedTool::MrBoxington.tool_name();
    let Some(selector) = spec.tools.get(tool) else {
        return MbxWrapperStatus::MissingPin;
    };
    if validate_exact_version(tool, selector).is_ok() {
        MbxWrapperStatus::Pinned {
            version: selector.clone(),
        }
    } else {
        MbxWrapperStatus::Invalid {
            problem: format!("inexact_pin:{tool}:{selector}"),
        }
    }
}

fn rust_option_selects_mbx(content: &str) -> bool {
    crate::toml_scan::parse_toml(content).is_ok_and(|doc| {
        doc.find(&["tools", "rust", "mr_boxington"])
            .iter()
            .any(|found| found.value == crate::toml_scan::TomlValue::Bool(true))
    })
}

#[must_use]
pub fn lock_tool_versions(content: &str) -> BTreeMap<String, String> {
    match inspect_mise_file(MISE_LOCK_FILE, Some(content)) {
        Ok(inspection) => inspection.spec.map(|spec| spec.tools).unwrap_or_default(),
        Err(_) => BTreeMap::new(),
    }
}

fn parse_tools(path: &str, content: &str) -> Result<MiseSpec, String> {
    if is_lock_file(path) {
        let lock = lockfile::parse_mise_lockfile(content)?;
        let tools = lock
            .tools
            .into_iter()
            .filter(|(_, entry)| !entry.version.is_empty())
            .map(|(name, entry)| (name, entry.version))
            .collect();
        return Ok(MiseSpec { tools });
    }
    config_tools::parse_config_tools(content).map(|tools| MiseSpec { tools })
}

fn is_lock_file(path: &str) -> bool {
    let name = file_name(path);
    name == MISE_LOCK_FILE || name.strip_prefix('.') == Some(MISE_LOCK_FILE)
}
