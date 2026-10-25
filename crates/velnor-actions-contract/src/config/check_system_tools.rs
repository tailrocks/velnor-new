use super::CheckPlatform;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckSystemTool {
    pub kind: CheckSystemToolKind,

    pub version: String,

    pub build: String,

    pub sdk_version: Option<String>,

    pub developer_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckSystemToolKind {
    Swift,

    Xcode,
}

#[must_use]
pub fn is_valid_check_system_version(value: &str) -> bool {
    value.len() <= super::super::MAX_CHECK_SYSTEM_VERSION_BYTES
        && value.split('.').count() >= 2
        && value.split('.').all(|component| {
            !component.is_empty()
                && component.bytes().all(|byte| byte.is_ascii_digit())
                && (component.len() == 1 || !component.starts_with('0'))
        })
}

#[must_use]
pub fn is_valid_check_system_build(kind: CheckSystemToolKind, value: &str) -> bool {
    if value.len() > 256 {
        return false;
    }
    match kind {
        CheckSystemToolKind::Xcode => {
            !value.is_empty()
                && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
                && value
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_digit())
                && value.bytes().any(|byte| byte.is_ascii_alphabetic())
        }
        CheckSystemToolKind::Swift => value.split_once(" clang-").is_some_and(|(swift, clang)| {
            swift
                .strip_prefix("swiftlang-")
                .is_some_and(is_valid_check_system_version)
                && is_valid_check_system_version(clang)
        }),
    }
}

impl CheckSystemTool {
    pub(crate) fn validate_set(
        tools: &[Self],
        platform: CheckPlatform,
        file: &str,
        key: &str,
    ) -> Result<(), ContractError> {
        validate_system_tools(tools, platform, file, key)
    }
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
        if !is_valid_check_system_version(&tool.version)
            || !is_valid_check_system_build(tool.kind, &tool.build)
        {
            return Err(bad("invalid_system_tool_pin"));
        }
        match tool.kind {
            CheckSystemToolKind::Swift
                if tool.sdk_version.is_some() || tool.developer_dir.is_some() =>
            {
                return Err(bad("swift_cannot_select_xcode_directory_or_sdk"));
            }
            CheckSystemToolKind::Xcode => {
                if tool
                    .sdk_version
                    .as_deref()
                    .is_none_or(|version| !is_valid_check_system_version(version))
                {
                    return Err(bad("xcode_requires_exact_sdk_version"));
                }
                if let Some(directory) = tool.developer_dir.as_deref()
                    && !safe_developer_dir(directory)
                {
                    return Err(bad("invalid_xcode_developer_directory"));
                }
            }
            CheckSystemToolKind::Swift => {}
        }
    }
    Ok(())
}

fn safe_developer_dir(value: &str) -> bool {
    value.len() <= super::super::MAX_CHECK_CONTAINER_PATH_BYTES
        && value.starts_with("/Applications/")
        && value.ends_with(".app/Contents/Developer")
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b' ')
        })
        && value
            .split('/')
            .skip(1)
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
