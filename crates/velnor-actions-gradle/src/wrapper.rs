//! Wrapper verification gates (stack contract §2.3).
//!
//! Gates 1-3 are static: they read `gradle/wrapper/gradle-wrapper.properties`
//! and the bytes of `gradle-wrapper.jar`. Gate 4 (execution through
//! `./gradlew`) belongs to the job renderer.

use std::collections::BTreeMap;
use std::fmt;

use sha2::{Digest, Sha256};

use crate::catalog::{self, GradleRelease};

/// Repository-relative location of the wrapper properties under a root.
pub const PROPERTIES_PATH: &str = "gradle/wrapper/gradle-wrapper.properties";
/// Repository-relative location of the wrapper jar under a root.
pub const JAR_PATH: &str = "gradle/wrapper/gradle-wrapper.jar";

const URL_PREFIX: &str = "https://services.gradle.org/distributions/gradle-";

/// Distribution flavour named by the URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavour {
    /// `-bin.zip`
    Bin,
    /// `-all.zip`
    All,
}

/// A named wrapper failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapperFinding {
    /// `gradle-wrapper.properties` is missing or has no `distributionUrl`.
    PropertiesMissing,
    /// The URL is not an official release distribution (host, snapshot, path).
    DistributionUrlUntrusted,
    /// `distributionSha256Sum` is absent.
    DistributionChecksumMissing,
    /// `distributionSha256Sum` differs from the official catalog digest.
    DistributionChecksumMismatch,
    /// The release is not in the catalog, so no digest can be proven.
    DistributionUncataloged,
    /// The jar digest is not an official wrapper-jar digest.
    WrapperJarUnverified,
}

impl WrapperFinding {
    /// Stable machine-readable code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::PropertiesMissing => "gradle_wrapper_properties_missing",
            Self::DistributionUrlUntrusted => "gradle_distribution_url_untrusted",
            Self::DistributionChecksumMissing => "gradle_distribution_checksum_missing",
            Self::DistributionChecksumMismatch => "gradle_distribution_checksum_mismatch",
            Self::DistributionUncataloged => "gradle_distribution_uncataloged",
            Self::WrapperJarUnverified => "gradle_wrapper_jar_unverified",
        }
    }
}

impl fmt::Display for WrapperFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Verified wrapper facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedWrapper {
    /// Gradle version from the URL.
    pub version: String,
    /// Distribution flavour.
    pub flavour: Flavour,
    /// The catalog row that proved the digests.
    pub release: &'static GradleRelease,
}

/// Parse `key=value` Java properties (comments, `\:` and `\=` escapes).
#[must_use]
pub fn parse_properties(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let unescape = |raw: &str| raw.trim().replace("\\:", ":").replace("\\=", "=");
            map.insert(unescape(key), unescape(value));
        }
    }
    map
}

/// Version and flavour of an official release distribution URL.
#[must_use]
pub fn parse_distribution_url(url: &str) -> Option<(String, Flavour)> {
    let rest = url.strip_prefix(URL_PREFIX)?;
    let (version, flavour) = if let Some(version) = rest.strip_suffix("-bin.zip") {
        (version, Flavour::Bin)
    } else {
        (rest.strip_suffix("-all.zip")?, Flavour::All)
    };
    crate::version::parse_gradle(version)
        .ok()
        .map(|version| (version, flavour))
}

/// SHA-256 of bytes as lower-case hex.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut out, byte| {
            out.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
            out.push(char::from_digit(u32::from(byte & 15), 16).unwrap_or('0'));
            out
        })
}

/// Run gates 1-3 over the properties text and the jar bytes.
///
/// # Errors
/// The first failing gate, in the order of the contract.
pub fn verify(properties: Option<&str>, jar: &[u8]) -> Result<VerifiedWrapper, WrapperFinding> {
    let props = parse_properties(properties.ok_or(WrapperFinding::PropertiesMissing)?);
    let url = props
        .get("distributionUrl")
        .ok_or(WrapperFinding::PropertiesMissing)?;
    let (version, flavour) =
        parse_distribution_url(url).ok_or(WrapperFinding::DistributionUrlUntrusted)?;
    let declared = props
        .get("distributionSha256Sum")
        .ok_or(WrapperFinding::DistributionChecksumMissing)?;
    let release = catalog::release(&version).ok_or(WrapperFinding::DistributionUncataloged)?;
    let expected = match flavour {
        Flavour::Bin => release.bin_sha256,
        Flavour::All => release.all_sha256,
    };
    if !declared.eq_ignore_ascii_case(expected) {
        return Err(WrapperFinding::DistributionChecksumMismatch);
    }
    if !catalog::is_official_wrapper_jar(&sha256_hex(jar)) {
        return Err(WrapperFinding::WrapperJarUnverified);
    }
    Ok(VerifiedWrapper {
        version,
        flavour,
        release,
    })
}
