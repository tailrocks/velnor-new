// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;

pub use velnor_actions_contract::MARKER_PREFIX;
pub const MARKER_SUFFIX: &str = "; edit .velnor/config.toml and regenerate.";

pub fn marker_for_version(version: &str) -> Result<String, RenderError> {
    validate_version(version)?;
    Ok(format!("{MARKER_PREFIX}{version}{MARKER_SUFFIX}"))
}

pub fn check_first_line(text: &str, version: &str) -> Result<(), RenderError> {
    let expected = marker_for_version(version)?;
    match text.lines().next() {
        Some(first) if first == expected => Ok(()),
        Some(first) => Err(RenderError::BadMarker {
            expected,
            found: first.to_owned(),
        }),
        None => Err(RenderError::BadMarker {
            expected,
            found: String::new(),
        }),
    }
}

pub fn with_marker(version: &str, body: &str) -> Result<String, RenderError> {
    let marker = marker_for_version(version)?;
    Ok(format!("{marker}\n{body}"))
}

pub fn validate_version(version: &str) -> Result<(), RenderError> {
    let ok = !version.is_empty()
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+' | b'_'));
    if ok {
        Ok(())
    } else {
        Err(RenderError::BadVersion(version.to_owned()))
    }
}
