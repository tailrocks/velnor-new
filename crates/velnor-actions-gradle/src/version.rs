//! Adapter-specific exact version grammars (HD-4a).
//!
//! `java` accepts `<vendor>-<X.Y.Z>[+<build>[.<n>]*][.LTS]` where the vendor
//! is one of [`JAVA_VENDORS`]; Gradle accepts `N.N` or `N.N.N`. Bare majors,
//! `lts`, `latest`, ranges and templates are rejected. Accepted strings are
//! lower-case ASCII over `[a-z0-9.+-]` only, so they can be written to the
//! single-line `$GITHUB_ENV` resolver output without escaping.

use std::fmt;

/// Vendors the Mise core `java` backend can install under an exact name.
pub const JAVA_VENDORS: [&str; 5] = [
    "temurin",
    "oracle-graalvm",
    "graalvm-community",
    "corretto",
    "zulu",
];

/// Why a version string was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionError {
    /// The vendor prefix is missing or not in [`JAVA_VENDORS`].
    Vendor,
    /// The `X.Y.Z` core is not exact (loose, leading zero, range, template).
    Core,
    /// The `+<build>` suffix is malformed.
    Build,
    /// A character outside `[A-Za-z0-9.+-]` (injection guard).
    Charset,
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
            Self::Vendor => "vendor",
            Self::Core => "core",
            Self::Build => "build",
            Self::Charset => "charset",
        };
        write!(f, "{}:{detail}", self.code())
    }
}

/// A validated exact JDK declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaVersion {
    /// Vendor from [`JAVA_VENDORS`].
    pub vendor: &'static str,
    /// Exact `X.Y.Z` core.
    pub core: String,
    /// `<build>[.<n>]*` after `+`, when present.
    pub build: Option<String>,
    /// Whether the `.LTS` suffix is present.
    pub lts: bool,
}

impl JavaVersion {
    /// Major version of the core (the Gradle toolchain `languageVersion`).
    #[must_use]
    pub fn major(&self) -> u32 {
        self.core
            .split('.')
            .next()
            .and_then(|part| part.parse().ok())
            .unwrap_or(0)
    }
}

impl fmt::Display for JavaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.vendor, self.core)?;
        if let Some(build) = &self.build {
            write!(f, "+{build}")?;
        }
        if self.lts {
            f.write_str(".LTS")?;
        }
        Ok(())
    }
}

/// Parse an exact JDK declaration.
///
/// # Errors
/// [`VersionError`] for anything outside the grammar.
pub fn parse_java(text: &str) -> Result<JavaVersion, VersionError> {
    if text.is_empty()
        || !text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
    {
        return Err(VersionError::Charset);
    }
    let vendor = JAVA_VENDORS
        .iter()
        .find(|vendor| {
            text.strip_prefix(**vendor)
                .is_some_and(|rest| rest.starts_with('-'))
        })
        .ok_or(VersionError::Vendor)?;
    let rest = &text[vendor.len() + 1..];
    let (rest, lts) = match rest.strip_suffix(".LTS") {
        Some(rest) => (rest, true),
        None => (rest, false),
    };
    let (core, build) = match rest.split_once('+') {
        Some((core, build)) => (core, Some(build)),
        None => (rest, None),
    };
    if !is_exact_triple(core) {
        return Err(VersionError::Core);
    }
    if let Some(build) = build
        && !build.split('.').all(is_number)
    {
        return Err(VersionError::Build);
    }
    Ok(JavaVersion {
        vendor,
        core: core.to_owned(),
        build: build.map(str::to_owned),
        lts,
    })
}

/// Parse a Gradle version: `N.N` or `N.N.N`.
///
/// # Errors
/// [`VersionError::Core`] for pre-releases, ranges and leading zeros.
pub fn parse_gradle(text: &str) -> Result<String, VersionError> {
    let parts: Vec<&str> = text.split('.').collect();
    if matches!(parts.len(), 2 | 3) && parts.iter().all(|part| is_number(part)) {
        Ok(text.to_owned())
    } else {
        Err(VersionError::Core)
    }
}

/// True for `0` or a digit run without a leading zero.
fn is_number(part: &str) -> bool {
    !part.is_empty()
        && part.chars().all(|c| c.is_ascii_digit())
        && (part == "0" || !part.starts_with('0'))
        && part.len() <= 9
}

fn is_exact_triple(core: &str) -> bool {
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| is_number(part))
}
