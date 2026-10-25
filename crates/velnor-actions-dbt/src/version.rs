use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionError {
    Charset,

    Core,

    Preview,
}

impl VersionError {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FusionVersion {
    pub major: u32,

    pub minor: u32,

    pub patch: u32,

    pub preview: Option<u32>,
}

impl Ord for FusionVersion {
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

fn component(text: &str) -> Option<u32> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if canonical { text.parse().ok() } else { None }
}

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
