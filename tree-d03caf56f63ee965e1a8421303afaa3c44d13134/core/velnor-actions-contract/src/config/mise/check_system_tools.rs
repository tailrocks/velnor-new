//! Exact pins for host-installed native tools; no installation configuration.
use super::CheckPlatform;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// A native tool whose existing host installation must match this pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckSystemTool {
    /// Closed native-tool identity.
    pub kind: CheckSystemToolKind,
    /// Exact installed version.
    pub version: String,
    /// Exact installed build identifier.
    pub build: String,
}

/// Native macOS tools available through fixed system executable paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckSystemToolKind {
    /// Swift compiler selected by `/usr/bin/xcrun`.
    Swift,
    /// Xcode selected by `/usr/bin/xcodebuild`.
    Xcode,
}

fn safe_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= super::super::MAX_CHECK_SYSTEM_VERSION_BYTES
        && value.split('.').all(|component| {
            !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn safe_build(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.contains("  ")
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+' | b' ')
        })
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
}

pub(super) fn validate_system_tools(
    tools: &[CheckSystemTool],
    platform: CheckPlatform,
    file: &str,
    key: &str,
) -> Result<(), ContractError> {
    let bad = |problem| ContractError::config(file, format!("{key}.system_tools"), problem);
    if !tools.is_empty() && platform == CheckPlatform::LinuxX64 {
        return Err(bad("system_tools_require_macos"));
    }
    if tools.windows(2).any(|pair| pair[0].kind >= pair[1].kind) {
        return Err(bad("system_tools_must_be_sorted_unique"));
    }
    for tool in tools {
        if !safe_version(&tool.version) || !safe_build(&tool.build) {
            return Err(bad("invalid_system_tool_pin"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
