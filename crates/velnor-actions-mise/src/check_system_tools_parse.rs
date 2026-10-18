//! Closed native Xcode and Swift output parsers.
use super::invalid;
use super::strict_text;
use crate::MiseError;
use std::path::Path;
use velnor_actions_contract::config::{
    CheckPlatform, CheckSystemToolKind, MAX_CHECK_CONTAINER_PATH_BYTES,
};

/// Parse the closed native version output grammar without executing anything.
/// # Errors
/// Rejects non-UTF8, diagnostics, incomplete identities and extra output.
pub fn parse_system_tool_version(
    kind: CheckSystemToolKind,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<(String, String), MiseError> {
    if !strict_text(stderr)?.is_empty() {
        return Err(invalid("system_tools", "unexpected_version_diagnostics"));
    }
    let text = strict_text(stdout)?;
    let lines: Vec<_> = text.lines().collect();
    if lines.len() != 2 {
        return Err(invalid("system_tools", "invalid_version_output"));
    }
    let (version, build) = match kind {
        CheckSystemToolKind::Xcode => (
            lines[0].strip_prefix("Xcode "),
            lines[1].strip_prefix("Build version "),
        ),
        CheckSystemToolKind::Swift => {
            let first = swift_product_line(lines[0])?;
            let (version, rest) = first
                .split_once(" (")
                .ok_or_else(|| invalid("system_tools", "invalid_swift_build"))?;
            let build = rest.strip_suffix(')');
            if !valid_swift_target(lines[1]) {
                return Err(invalid("system_tools", "invalid_swift_target"));
            }
            (Some(version), build)
        }
    };
    match (version, build) {
        (Some(v), Some(b)) if valid_version(v) && valid_build(kind, b) => {
            Ok((v.to_owned(), b.to_owned()))
        }
        _ => Err(invalid("system_tools", "invalid_product_identity")),
    }
}

fn swift_product_line(line: &str) -> Result<&str, MiseError> {
    if let Some(value) = line.strip_prefix("Apple Swift version ") {
        return Ok(value);
    }
    let driver = line
        .strip_prefix("swift-driver version: ")
        .ok_or_else(|| invalid("system_tools", "not_apple_swift"))?;
    let (version, value) = driver
        .split_once(" Apple Swift version ")
        .ok_or_else(|| invalid("system_tools", "missing_apple_swift"))?;
    if !valid_version(version) {
        return Err(invalid("system_tools", "invalid_swift_driver"));
    }
    Ok(value)
}

fn valid_swift_target(line: &str) -> bool {
    ["Target: arm64-apple-macosx", "Target: x86_64-apple-macosx"]
        .iter()
        .any(|prefix| line.strip_prefix(prefix).is_some_and(valid_version))
}

pub(super) fn valid_proof_target(
    platform: CheckPlatform,
    target: Option<&str>,
    swift: bool,
) -> bool {
    if !swift {
        return target.is_none();
    }
    let prefix = if platform == CheckPlatform::MacosArm64 {
        "arm64-apple-macosx"
    } else {
        "x86_64-apple-macosx"
    };
    target
        .and_then(|value| value.strip_prefix(prefix))
        .is_some_and(valid_version)
}

pub(super) fn valid_version(value: &str) -> bool {
    value.len() <= velnor_actions_contract::MAX_CHECK_SYSTEM_VERSION_BYTES
        && value.split('.').count() >= 2
        && value.split('.').all(|part| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

pub(super) fn valid_build(kind: CheckSystemToolKind, value: &str) -> bool {
    match kind {
        CheckSystemToolKind::Xcode => {
            !value.is_empty()
                && value.bytes().all(|b| b.is_ascii_alphanumeric())
                && value.bytes().next().is_some_and(|b| b.is_ascii_digit())
                && value.bytes().any(|b| b.is_ascii_alphabetic())
        }
        CheckSystemToolKind::Swift => value.split_once(" clang-").is_some_and(|(swift, clang)| {
            swift.strip_prefix("swiftlang-").is_some_and(valid_version) && valid_version(clang)
        }),
    }
}

pub(super) fn absolute_identity(value: &str) -> bool {
    value.len() <= MAX_CHECK_CONTAINER_PATH_BYTES
        && Path::new(value).is_absolute()
        && !value.contains(['\n', '\r', '\0'])
        && Path::new(value).components().all(|c| {
            matches!(
                c,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
}
