//! First-line generator-version marker shared by every generated file.
//!
//! The marker carries the exact generator version and no dates.

use crate::RenderError;

/// Marker text before the version (re-exported from the contract crate).
pub use velnor_actions_contract::MARKER_PREFIX;
/// Marker text after the version.
pub const MARKER_SUFFIX: &str = "; edit .velnor/config.toml and regenerate.";

/// Build the exact first-line marker for a generator version.
///
/// # Errors
///
/// Returns [`RenderError::BadVersion`] when the version is malformed.
pub fn marker_for_version(version: &str) -> Result<String, RenderError> {
    validate_version(version)?;
    Ok(format!("{MARKER_PREFIX}{version}{MARKER_SUFFIX}"))
}

/// Check that `text` starts with the exact marker line for `version`.
///
/// # Errors
///
/// Returns [`RenderError::BadVersion`] for a malformed version or
/// [`RenderError::BadMarker`] when the first line differs.
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

/// Prepend the marker line to a rendered body.
///
/// # Errors
///
/// Returns [`RenderError::BadVersion`] when the version is malformed.
pub fn with_marker(version: &str, body: &str) -> Result<String, RenderError> {
    let marker = marker_for_version(version)?;
    Ok(format!("{marker}\n{body}"))
}

/// Reject empty versions and anything outside ASCII `A-Za-z0-9._+-`.
///
/// # Errors
///
/// Returns [`RenderError::BadVersion`] when the version is malformed.
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
