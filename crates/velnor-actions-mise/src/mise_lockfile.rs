// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::catalog::{PinnedTool, ToolCatalog};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockEntry {
    pub version: String,
    pub checksums: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MiseLockfile {
    pub tools: BTreeMap<String, LockEntry>,
    pub entries: BTreeMap<String, Vec<LockEntry>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallCoverage {
    Verified,
    VersionDrift { locked: String },
    MissingEntry,
    NoChecksums,
    MissingPlatform { locked_platforms: Vec<String> },
    CorruptChecksum { observed: String },
}

impl InstallCoverage {
    #[must_use]
    pub fn is_blocking(&self) -> bool {
        matches!(
            self,
            Self::MissingPlatform { .. } | Self::CorruptChecksum { .. }
        )
    }
}

#[must_use]
pub fn mise_platform_for_target(triple: &str) -> Option<&'static str> {
    match triple {
        "x86_64-unknown-linux-gnu" => Some("linux-x64"),
        "aarch64-apple-darwin" => Some("macos-arm64"),
        "x86_64-apple-darwin" => Some("macos-x64"),
        _ => None,
    }
}

pub fn parse_mise_lockfile(text: &str) -> Result<MiseLockfile, String> {
    let mut lock = MiseLockfile::default();
    let mut tool: Option<String> = None;
    let mut platform: Option<String> = None;
    for (index, line) in text.lines().enumerate() {
        let code = line.trim();
        if code.is_empty() || code.starts_with('#') {
            continue;
        }
        if let Some(header) = parse_header(code, index)? {
            match header {
                Header::Tool(key) => {
                    let blank = LockEntry {
                        version: String::new(),
                        checksums: BTreeMap::new(),
                    };
                    lock.entries
                        .entry(key.clone())
                        .or_default()
                        .push(blank.clone());
                    lock.tools.entry(key.clone()).or_insert(blank);
                    tool = Some(key);
                    platform = None;
                }
                Header::Platform(key, name) => {
                    if tool.as_ref() != Some(&key) || !lock.tools.contains_key(&key) {
                        return Err(line_problem(index, "platform_without_tool"));
                    }
                    platform = Some(name);
                }
                Header::ToolOptions(key) => {
                    if tool.as_ref() != Some(&key) || !lock.tools.contains_key(&key) {
                        return Err(line_problem(index, "options_without_tool"));
                    }
                    platform = None;
                }
                Header::Other => {
                    tool = None;
                    platform = None;
                }
            }
            continue;
        }
        let (key, value) = split_setting(code, index)?;
        match (tool.as_ref(), platform.as_ref(), key) {
            (Some(name), None, "version") => {
                let value = expect_quoted_string(value, index)?;
                let current = lock
                    .entries
                    .get_mut(name)
                    .and_then(|list| list.last_mut())
                    .ok_or_else(|| line_problem(index, "version_without_tool"))?;
                if !current.version.is_empty() {
                    return Err(line_problem(index, "duplicate_key"));
                }
                current.version.clone_from(&value);
                if let Some(summary) = lock.tools.get_mut(name) {
                    summary.version = value;
                }
            }
            (Some(name), Some(platform), "checksum") => {
                let value = expect_quoted_string(value, index)?;
                let current = lock
                    .entries
                    .get_mut(name)
                    .and_then(|list| list.last_mut())
                    .ok_or_else(|| line_problem(index, "checksum_without_tool"))?;
                if current.checksums.contains_key(platform) {
                    return Err(line_problem(index, "duplicate_key"));
                }
                current.checksums.insert(platform.clone(), value.clone());
                if let Some(summary) = lock.tools.get_mut(name) {
                    summary.checksums.insert(platform.clone(), value);
                }
            }
            _ => {}
        }
    }
    Ok(lock)
}

enum Header {
    Tool(String),
    Platform(String, String),
    ToolOptions(String),
    Other,
}

enum SettingValue {
    Quoted(String),
    Other,
}

fn parse_header(code: &str, index: usize) -> Result<Option<Header>, String> {
    if !code.starts_with('[') {
        return Ok(None);
    }
    if let Some(key) = code
        .strip_prefix("[[tools.")
        .and_then(|rest| rest.strip_suffix("]]"))
    {
        return Ok(Some(Header::Tool(unquote(key.trim(), index)?)));
    }
    let Some(inner) = code
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    else {
        return Err(line_problem(index, "unterminated_section"));
    };
    let Some(rest) = inner.strip_prefix("tools.") else {
        return Ok(Some(Header::Other));
    };
    if let Some(key) = rest.strip_suffix(".options") {
        return Ok(Some(Header::ToolOptions(unquote(key.trim(), index)?)));
    }
    let Some((key, platform)) = rest.rsplit_once(r#"."platforms."#) else {
        return Ok(Some(Header::Other));
    };
    let platform = platform
        .strip_suffix('"')
        .ok_or_else(|| line_problem(index, "unterminated_platform"))?;
    if key.is_empty() || platform.is_empty() {
        return Err(line_problem(index, "empty_platform_key"));
    }
    Ok(Some(Header::Platform(
        unquote(key.trim(), index)?,
        platform.to_owned(),
    )))
}

fn unquote(key: &str, index: usize) -> Result<String, String> {
    if !key.starts_with('"') {
        if key.is_empty() {
            return Err(line_problem(index, "empty_tool_key"));
        }
        return Ok(key.to_owned());
    }
    if key.len() >= 2 && key.ends_with('"') {
        return Ok(key[1..key.len() - 1].to_owned());
    }
    Err(line_problem(index, "unterminated_key"))
}

fn split_setting(code: &str, index: usize) -> Result<(&str, SettingValue), String> {
    let (key, value) = code
        .split_once('=')
        .ok_or_else(|| line_problem(index, "expected_key_equals_value"))?;
    let key = key.trim();
    let trimmed = value.trim();
    let value = if let Some(value) = trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
                .filter(|inner| !inner.contains('\''))
        }) {
        Some(SettingValue::Quoted(value.to_owned()))
    } else {
        let integer = !trimmed.is_empty() && trimmed.bytes().all(|b| b.is_ascii_digit());
        let array = trimmed.starts_with('[') && trimmed.ends_with(']');
        let inline_table = trimmed.starts_with('{') && trimmed.ends_with('}');
        let boolean = matches!(trimmed, "true" | "false");
        (integer || array || inline_table || boolean).then_some(SettingValue::Other)
    };
    match (key.is_empty(), value) {
        (false, Some(value)) => Ok((key, value)),
        _ => Err(line_problem(index, "expected_quoted_string")),
    }
}

fn expect_quoted_string(value: SettingValue, index: usize) -> Result<String, String> {
    match value {
        SettingValue::Quoted(value) => Ok(value),
        SettingValue::Other => Err(line_problem(index, "expected_quoted_string")),
    }
}

fn line_problem(index: usize, problem: &str) -> String {
    format!("line {}: {problem}", index.saturating_add(1))
}

fn lock_keys_for(tool: PinnedTool, catalog: &ToolCatalog) -> Vec<String> {
    let mut keys = vec![tool.tool_name().to_owned()];
    let spec = catalog.tool_spec(tool);
    if let Some(prefix) = spec.split_once('@').map(|(head, _)| head.to_owned())
        && prefix != keys[0]
    {
        keys.push(prefix);
    }
    keys
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallSubject {
    pub display: String,
    pub expected_version: String,
    pub lock_key: String,
}

#[must_use]
pub fn subject_for_install_spec(spec: &str, catalog: &ToolCatalog) -> Option<InstallSubject> {
    let (key, version) = spec.split_once('@')?;
    if key.is_empty() || version.is_empty() {
        return None;
    }
    let tool = PinnedTool::ALL.iter().find(|tool| {
        lock_keys_for(**tool, catalog)
            .iter()
            .any(|known| known == key)
    })?;
    if version != catalog.version(*tool) {
        return None;
    }
    Some(InstallSubject {
        display: format!("{}@{}", tool.tool_name(), catalog.version(*tool)),
        expected_version: catalog.version(*tool).to_owned(),
        lock_key: key.to_owned(),
    })
}

fn is_enforceable_checksum(value: &str) -> bool {
    value
        .split_once(':')
        .is_some_and(|(scheme, hex)| !scheme.is_empty() && !hex.is_empty() && is_hex(hex))
}

fn is_hex(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_hexdigit())
}

#[must_use]
pub fn audit_install_coverage(
    lock: &MiseLockfile,
    subjects: &[InstallSubject],
    platform: &str,
) -> Vec<InstallCoverage> {
    subjects
        .iter()
        .map(|subject| coverage_for_subject(lock, subject, platform))
        .collect()
}

fn coverage_for_subject(
    lock: &MiseLockfile,
    subject: &InstallSubject,
    platform: &str,
) -> InstallCoverage {
    let entry = lock
        .entries
        .get(&subject.lock_key)
        .and_then(|list| {
            list.iter()
                .find(|item| item.version == subject.expected_version)
                .or_else(|| list.last())
        })
        .or_else(|| lock.tools.get(&subject.lock_key));
    let Some(entry) = entry else {
        return InstallCoverage::MissingEntry;
    };
    if entry.version != subject.expected_version {
        return InstallCoverage::VersionDrift {
            locked: entry.version.clone(),
        };
    }
    match entry.checksums.get(platform) {
        Some(checksum) if is_enforceable_checksum(checksum) => InstallCoverage::Verified,
        Some(checksum) => InstallCoverage::CorruptChecksum {
            observed: checksum.clone(),
        },
        None if entry.checksums.is_empty() => InstallCoverage::NoChecksums,
        None => InstallCoverage::MissingPlatform {
            locked_platforms: entry.checksums.keys().cloned().collect(),
        },
    }
}
