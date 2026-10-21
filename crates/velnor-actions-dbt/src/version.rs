//! The dbt-fusion exact version grammar (HD-4a): `X.Y.Z-preview.N` or the
//! stable `X.Y.Z`.
//!
//! Fusion ships as `<major>.<minor>.<patch>-preview.<n>` and, since 2.0.9, as
//! plain `<major>.<minor>.<patch>`; `latest`, a range or a template is not an
//! exact pin. Both forms use the same CDN archive name. Accepted strings are lower-case ASCII
//! over `[a-z0-9.-]`, so they can be written to the single-line `$GITHUB_ENV`
//! resolver output without escaping.

use std::fmt;

/// Why a version string was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionError {
    /// A character outside `[A-Za-z0-9.-]` (injection guard).
    Charset,
    /// The `X.Y.Z` core is not exact (loose, leading zero, range, template).
    Core,
    /// The `-preview.N` suffix is missing or malformed.
    Preview,
}

impl VersionError {
    /// Stable finding code shared by every project tool version failure.
    #[must_use]
    pub const fn code(self) -> &'static str {
        "project_tool_version_not_exact"
    }
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let detail = match self {
            Self::Charset => "charset",
            Self::Core => "core",
            Self::Preview => "preview",
        };
        write!(f, "{}:{detail}", self.code())
    }
}

/// A validated exact dbt-fusion declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FusionVersion {
    /// Major component.
    pub major: u32,
    /// Minor component.
    pub minor: u32,
    /// Patch component.
    pub patch: u32,
    /// Preview number after `-preview.`; `None` for a stable release.
    pub preview: Option<u32>,
}

impl Ord for FusionVersion {
    /// A stable release orders after every preview of the same `X.Y.Z`.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let key = |v: &Self| (v.major, v.minor, v.patch, v.preview.is_none(), v.preview);
        key(self).cmp(&key(other))
    }
}

impl PartialOrd for FusionVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for FusionVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(preview) = self.preview {
            write!(f, "-preview.{preview}")?;
        }
        Ok(())
    }
}

/// Parse one canonical decimal component (no sign, no leading zero).
fn component(text: &str) -> Option<u32> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if canonical { text.parse().ok() } else { None }
}

/// Parse `X.Y.Z-preview.N` or the stable `X.Y.Z`.
///
/// # Errors
///
/// [`VersionError`] naming the failing part.
pub fn parse_fusion_version(text: &str) -> Result<FusionVersion, VersionError> {
    if !text
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
    {
        return Err(VersionError::Charset);
    }
    let (core, suffix) = match text.split_once('-') {
        Some((core, suffix)) => (core, Some(suffix)),
        None => (text, None),
    };
    let mut parts = core.split('.');
    let (Some(major), Some(minor), Some(patch), None) = (
        parts.next().and_then(component),
        parts.next().and_then(component),
        parts.next().and_then(component),
        parts.next(),
    ) else {
        return Err(VersionError::Core);
    };
    let preview = match suffix {
        None => None,
        Some(suffix) => Some(
            suffix
                .strip_prefix("preview.")
                .and_then(component)
                .ok_or(VersionError::Preview)?,
        ),
    };
    Ok(FusionVersion {
        major,
        minor,
        patch,
        preview,
    })
}
