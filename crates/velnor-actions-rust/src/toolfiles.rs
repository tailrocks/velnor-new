// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

pub const RUST_TOOLCHAIN_FILE: &str = "rust-toolchain.toml";

pub const RUST_TOOLCHAIN_LEGACY_FILE: &str = "rust-toolchain";

pub const OWNED_SYMBOLS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    RUST_TOOLCHAIN_FILE,
    RUST_TOOLCHAIN_LEGACY_FILE,
];

pub const FOREIGN_TOOL_FILES: &[&str] = &["mise.toml", "mise.lock"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionBroadening {
    Lockfile,
    RootConfig,
}

#[must_use]
pub fn selection_broadening(path: &str) -> Option<SelectionBroadening> {
    if path == "Cargo.lock" || path.ends_with("/Cargo.lock") {
        return Some(SelectionBroadening::Lockfile);
    }
    if path == "Cargo.toml" || path == ".cargo/config.toml" || path == ".cargo/config" {
        return Some(SelectionBroadening::RootConfig);
    }
    None
}

#[must_use]
pub fn is_known_toolfile(path: &str) -> bool {
    path == RUST_TOOLCHAIN_FILE
        || path == RUST_TOOLCHAIN_LEGACY_FILE
        || FOREIGN_TOOL_FILES.contains(&path)
}

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
pub struct ToolchainSpec {
    pub channel: Option<String>,
    pub profile: Option<String>,
    pub components: Vec<String>,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolchainInspection {
    pub file: String,
    pub spec: Option<ToolchainSpec>,
    pub findings: Vec<ToolFinding>,
}

impl ToolchainInspection {
    fn missing(path: &str) -> Self {
        Self {
            file: path.to_owned(),
            spec: None,
            findings: vec![ToolFinding {
                code: MISSING_RECOMMENDED_INPUT.to_owned(),
                file: path.to_owned(),
                observed: None,
                recommendation: format!(
                    "add a user-authored [toolchain] channel pin in a new \
                     {path}; Velnor never creates it"
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
                    "fix {problem} manually in {path}; generation continues \
                     with Velnor's pinned Rust"
                ),
            }],
        }
    }

    fn present(path: &str, spec: ToolchainSpec) -> Self {
        let findings = if spec.channel.as_deref().is_none_or(str::is_empty) {
            vec![ToolFinding {
                code: MISSING_RECOMMENDED_INPUT.to_owned(),
                file: path.to_owned(),
                observed: Some("no [toolchain] channel".to_owned()),
                recommendation: format!(
                    "add channel = \"<pinned stable>\" under [toolchain] in \
                     {path}; Velnor never edits it"
                ),
            }]
        } else {
            Vec::new()
        };
        Self {
            file: path.to_owned(),
            spec: Some(spec),
            findings,
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
        Some(crate::STACK_ID)
    } else if FOREIGN_TOOL_FILES.contains(&name) {
        Some("mise")
    } else {
        None
    }
}

#[must_use]
pub fn is_owned_tool_file(path: &str) -> bool {
    matches!(
        file_name(path),
        RUST_TOOLCHAIN_FILE | RUST_TOOLCHAIN_LEGACY_FILE
    )
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn extract_rust_toolchain_channel(path: &str, bytes: &str) -> Result<String, String> {
    if !is_owned_tool_file(path) {
        return Err(format!("tool_file_not_owned:{path}"));
    }
    let channel = parse_toolchain_for_path(path, bytes)?
        .channel
        .unwrap_or_default();
    if channel.is_empty() {
        return Err("missing_toolchain_channel".to_owned());
    }
    if is_exact_semver(&channel) {
        return Ok(channel);
    }
    if let Some((ver, target)) = channel.split_once('-')
        && is_exact_semver(ver)
        && is_target_triple(target)
    {
        return Ok(ver.to_owned());
    }
    Err(channel)
}

fn is_exact_semver(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !(part.is_empty() || part.len() > 1 && part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn is_target_triple(target: &str) -> bool {
    target.split('-').count() >= 3
        && target
            .split('-')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
}

pub fn inspect_toolchain_file(
    path: &str,
    content: Option<&str>,
) -> Result<ToolchainInspection, ToolInspectError> {
    if !is_owned_tool_file(path) {
        return Err(ToolInspectError::NotOwned {
            path: path.to_owned(),
        });
    }
    let Some(text) = content else {
        return Ok(ToolchainInspection::missing(path));
    };
    match parse_toolchain_for_path(path, text) {
        Ok(spec) => Ok(ToolchainInspection::present(path, spec)),
        Err(problem) => Ok(ToolchainInspection::invalid(path, &problem)),
    }
}

fn parse_toolchain_for_path(path: &str, content: &str) -> Result<ToolchainSpec, String> {
    if file_name(path) == RUST_TOOLCHAIN_LEGACY_FILE {
        let trimmed = content.trim();
        if trimmed.starts_with('[') {
            parse_toolchain(content)
        } else if !trimmed.is_empty()
            && !trimmed.contains('\n')
            && !trimmed.contains('\r')
            && !trimmed.chars().any(char::is_whitespace)
        {
            Ok(ToolchainSpec {
                channel: Some(trimmed.to_owned()),
                profile: None,
                components: Vec::new(),
                targets: Vec::new(),
            })
        } else {
            Err("line 1: malformed_legacy_toolchain".to_owned())
        }
    } else {
        parse_toolchain(content)
    }
}

fn parse_toolchain(content: &str) -> Result<ToolchainSpec, String> {
    let mut spec = ToolchainSpec::default();
    let mut in_toolchain = false;
    for (index, line) in content.lines().enumerate() {
        let code = crate::scan::strip_comment(line).trim();
        if code.is_empty() {
            continue;
        }
        if let Some(section) = parse_section(code) {
            in_toolchain = section == "toolchain";
            continue;
        }
        if code.starts_with('[') {
            return Err(line_problem(index, "unterminated_section"));
        }
        if !in_toolchain {
            continue;
        }
        let (key, value) = split_setting(code, index)?;
        match key {
            "channel" => spec.channel = Some(parse_string(value, index)?),
            "profile" => spec.profile = Some(parse_string(value, index)?),
            "components" => spec.components = parse_string_list(value, index)?,
            "targets" => spec.targets = parse_string_list(value, index)?,
            _ => {}
        }
    }
    spec.components.sort();
    spec.components.dedup();
    spec.targets.sort();
    spec.targets.dedup();
    Ok(spec)
}

fn parse_section(code: &str) -> Option<&str> {
    let section = code.strip_prefix('[')?.strip_suffix(']')?;
    Some(section.trim())
}

fn split_setting(code: &str, index: usize) -> Result<(&str, &str), String> {
    let (key, value) = code
        .split_once('=')
        .ok_or_else(|| line_problem(index, "expected_key_equals_value"))?;
    let key = key.trim();
    if key.is_empty() || value.trim().is_empty() {
        return Err(line_problem(index, "expected_key_equals_value"));
    }
    Ok((key, value.trim()))
}

fn parse_string(value: &str, index: usize) -> Result<String, String> {
    let quote = value.chars().next().unwrap_or('\0');
    if (quote != '"' && quote != '\'') || value.len() < 2 || !value.ends_with(quote) {
        return Err(line_problem(index, "unterminated_string"));
    }
    Ok(value[1..value.len() - 1].to_owned())
}

fn parse_string_list(value: &str, index: usize) -> Result<Vec<String>, String> {
    let inner = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| line_problem(index, "unterminated_list"))?;
    let mut items = Vec::new();
    for item in inner.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        items.push(parse_string(item, index)?);
    }
    Ok(items)
}

fn line_problem(index: usize, problem: &str) -> String {
    format!("line {}: {problem}", index.saturating_add(1))
}
