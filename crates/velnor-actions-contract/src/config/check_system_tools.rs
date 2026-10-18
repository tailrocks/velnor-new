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
    /// Exact macOS SDK version for Xcode; absent for Swift.
    pub sdk_version: Option<String>,
    /// Canonical Xcode developer directory override; absent uses host selection.
    pub developer_dir: Option<String>,
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

/// Validate a canonical dotted numeric system-tool version shared with runtime probes.
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

/// Validate a kind-specific complete build identity shared with runtime probes.
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
    /// Validate an ordered native-tool pin set against its host platform.
    /// # Errors
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

#[cfg(test)]
mod tests {
    use super::*;

    fn swift() -> CheckSystemTool {
        CheckSystemTool {
            kind: CheckSystemToolKind::Swift,
            version: "6.2.1".to_owned(),
            build: "swiftlang-6.2.1.1.1 clang-1700.0.13.3".to_owned(),
            sdk_version: None,
            developer_dir: None,
        }
    }

    #[test]
    fn system_pins_require_macos_safe_values_and_unique_sorted_kinds() {
        let tools = vec![swift()];
        assert!(
            validate_system_tools(&tools, CheckPlatform::MacosArm64, "config", "checks[0]").is_ok()
        );
        assert!(
            validate_system_tools(&tools, CheckPlatform::LinuxX64, "config", "checks[0]").is_err()
        );
        let duplicate = vec![swift(), swift()];
        assert!(
            validate_system_tools(&duplicate, CheckPlatform::MacosX64, "config", "checks[0]")
                .is_err()
        );
        for bad in [
            "",
            "$(command)",
            "${{ env.X }}",
            "-flag",
            "a b",
            "a\nb",
            "https://example.com",
        ] {
            let mut invalid = swift();
            invalid.version = bad.to_owned();
            assert!(
                validate_system_tools(&[invalid], CheckPlatform::MacosArm64, "config", "checks[0]")
                    .is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn schema_version_and_build_grammar_matches_runtime_identity_parser() {
        for version in ["27.0", "27.1", "6.4.0.34.1"] {
            assert!(is_valid_check_system_version(version), "{version}");
        }
        for version in ["27", "027.0", "27.00", "27.x", "27.0 "] {
            assert!(!is_valid_check_system_version(version), "{version}");
        }

        for (kind, build) in [
            (
                CheckSystemToolKind::Swift,
                "swiftlang-6.4.0.34.1 clang-2100.3.34.1",
            ),
            (CheckSystemToolKind::Xcode, "27A9275"),
        ] {
            assert!(is_valid_check_system_build(kind, build), "{build}");
        }
        for (kind, build) in [
            (CheckSystemToolKind::Swift, "swiftlang-6.2.1.1.1"),
            (CheckSystemToolKind::Swift, "swiftlang-6.4 clang-2100.3 bad"),
            (CheckSystemToolKind::Xcode, "27.1 RC"),
            (CheckSystemToolKind::Xcode, "27A-9275"),
        ] {
            assert!(!is_valid_check_system_build(kind, build), "{build}");
        }
    }

    #[test]
    fn xcode_pin_requires_safe_path_and_exact_sdk() {
        let xcode = xcode();
        assert!(
            validate_system_tools(
                std::slice::from_ref(&xcode),
                CheckPlatform::MacosArm64,
                "config",
                "checks[0]"
            )
            .is_ok()
        );
        for directory in [
            "/Applications/../tmp/Xcode.app/Contents/Developer",
            "/tmp/Xcode.app/Contents/Developer",
            "/Applications/Xcode.app/Contents/Developer/../SharedFrameworks",
            "/Applications/Xcode.app/Contents/Developer;touch",
        ] {
            let mut invalid = xcode.clone();
            invalid.developer_dir = Some(directory.to_owned());
            assert!(
                validate_system_tools(&[invalid], CheckPlatform::MacosArm64, "config", "checks[0]")
                    .is_err()
            );
        }
        let mut missing_sdk = xcode.clone();
        missing_sdk.sdk_version = None;
        assert!(
            validate_system_tools(
                &[missing_sdk],
                CheckPlatform::MacosArm64,
                "config",
                "checks[0]"
            )
            .is_err()
        );
        let mut swift_override = swift();
        swift_override.developer_dir = xcode.developer_dir.clone();
        assert!(
            validate_system_tools(
                &[swift_override],
                CheckPlatform::MacosArm64,
                "config",
                "checks[0]"
            )
            .is_err()
        );
    }

    #[test]
    fn xcode_version_sdk_and_build_match_runtime_grammar() {
        for (field, values) in [
            ("version", ["27.0", "27.1", "27", "027.0", "27.00"]),
            ("sdk", ["27.0", "27.1", "27", "027.0", "27.00"]),
        ] {
            for value in values {
                let mut candidate = xcode();
                if field == "version" {
                    candidate.version = value.to_owned();
                } else {
                    candidate.sdk_version = Some(value.to_owned());
                }
                assert_eq!(
                    validate_system_tools(
                        &[candidate],
                        CheckPlatform::MacosArm64,
                        "config",
                        "checks[0]"
                    )
                    .is_ok(),
                    is_valid_check_system_version(value),
                    "Xcode {field} {value}"
                );
            }
        }
        for build in ["27A9275", "27.0", "27A-9275", "27.1 RC"] {
            let mut candidate = xcode();
            candidate.build = build.to_owned();
            assert_eq!(
                validate_system_tools(
                    &[candidate],
                    CheckPlatform::MacosArm64,
                    "config",
                    "checks[0]"
                )
                .is_ok(),
                is_valid_check_system_build(CheckSystemToolKind::Xcode, build),
                "Xcode build {build}"
            );
        }
    }

    #[test]
    fn tool_selection_kinds_are_sorted_and_unique() {
        let xcode = xcode();
        assert!(
            validate_system_tools(
                &[swift(), swift()],
                CheckPlatform::MacosArm64,
                "config",
                "checks[0]"
            )
            .is_err()
        );
        assert!(
            validate_system_tools(
                &[xcode.clone(), xcode.clone()],
                CheckPlatform::MacosArm64,
                "config",
                "checks[0]"
            )
            .is_err()
        );
        assert!(
            validate_system_tools(
                &[swift(), xcode],
                CheckPlatform::MacosArm64,
                "config",
                "checks[0]"
            )
            .is_ok()
        );
    }

    fn xcode() -> CheckSystemTool {
        CheckSystemTool {
            kind: CheckSystemToolKind::Xcode,
            version: "27.1".to_owned(),
            build: "27A9275".to_owned(),
            sdk_version: Some("27.0".to_owned()),
            developer_dir: Some(
                "/Applications/Xcode_27.1_Release_Candidate.app/Contents/Developer".to_owned(),
            ),
        }
    }
    #[test]
    fn complete_swift_compiler_tuple_is_pinned_without_shell_syntax() {
        let mut pin = swift();
        pin.version = "6.4".to_owned();
        pin.build = "swiftlang-6.4.0.34.1 clang-2100.3.34.1".to_owned();
        assert!(
            validate_system_tools(
                &[pin.clone()],
                CheckPlatform::MacosArm64,
                "config",
                "checks[0]"
            )
            .is_ok()
        );
        for build in [
            "",
            " leading",
            "trailing ",
            "double  spaces",
            "a\nb",
            "${{ env.X }}",
            "$(id)",
            "a;b",
            "a|b",
            "a\tb",
        ] {
            pin.build = build.to_owned();
            assert!(
                validate_system_tools(
                    &[pin.clone()],
                    CheckPlatform::MacosArm64,
                    "config",
                    "checks[0]"
                )
                .is_err(),
                "{build:?}"
            );
        }
        assert!(!is_valid_check_system_build(
            CheckSystemToolKind::Swift,
            &"a".repeat(257)
        ));
        for version in ["", ".6", "6.", "6..4", "swift-6.4", "6 4", "6;4"] {
            assert!(!is_valid_check_system_version(version), "{version:?}");
        }
    }
}
