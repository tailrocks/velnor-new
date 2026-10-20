//! Tag-driven publication config (`[[publish.bundles]]`) and the tag grammar.
//!
//! Pure data plus validation: no subprocess, registry, or YAML access.
//! Versioning is independent `SemVer` per artifact. The source of truth is the
//! artifact's own manifest, read through [`VersionSource`], or an inline
//! `version` literal in this config. A release tag is
//! `<artifact-id>-v<X.Y.Z>` ([`ReleaseTag`]).

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Push-tag glob that admits release tags (`<id>-v<X.Y.Z>` shape only).
///
/// Coarse by design: the authority job re-parses the tag with the strict
/// grammar, so the glob only keeps unrelated tags from starting a run.
pub const RELEASE_TAG_GLOB: &str = "*-v[0-9]*.[0-9]*.[0-9]*";

/// Longest accepted artifact id.
const MAX_ID_BYTES: usize = 64;
/// Most bundles one repository may declare.
const MAX_BUNDLES: usize = 64;
/// Most paths one bundle may list.
const MAX_PATHS: usize = 64;

/// Manifest family a version is read from, chosen by the file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionSource {
    /// `Cargo.toml`: first literal `version` of `[package]` or `[workspace.package]`.
    CargoToml,
    /// `gradle.properties`: the `version=X.Y.Z` line.
    GradleProperties,
    /// `package.json`: the top-level `version` string.
    PackageJson,
}

impl VersionSource {
    /// Pick the source from a repo-relative file name (`None` when unsupported).
    #[must_use]
    pub fn for_path(path: &str) -> Option<Self> {
        match path.rsplit('/').next()? {
            "Cargo.toml" => Some(Self::CargoToml),
            "gradle.properties" => Some(Self::GradleProperties),
            "package.json" => Some(Self::PackageJson),
            _ => None,
        }
    }

    /// Extract the strict `X.Y.Z` version from manifest text.
    ///
    /// The generated authority job applies the same rule at run time, so a
    /// manifest that passes here is read identically there.
    #[must_use]
    pub fn read(self, text: &str) -> Option<String> {
        match self {
            Self::CargoToml => read_cargo_version(text),
            Self::GradleProperties => text
                .lines()
                .find_map(|line| {
                    line.strip_prefix("version=")
                        .filter(|v| is_strict_semver(v))
                })
                .map(str::to_owned),
            Self::PackageJson => {
                let value: serde_json::Value = serde_json::from_str(text).ok()?;
                let version = value.get("version")?.as_str()?;
                is_strict_semver(version).then(|| version.to_owned())
            }
        }
    }
}

/// First literal `version = "X.Y.Z"` inside `[package]` or `[workspace.package]`.
fn read_cargo_version(text: &str) -> Option<String> {
    let mut in_package = false;
    for line in text.lines() {
        if line.starts_with('[') {
            let header = line.trim_end_matches(' ');
            in_package = header == "[package]" || header == "[workspace.package]";
        } else if in_package && let Some(version) = cargo_version_line(line) {
            return Some(version.to_owned());
        }
    }
    None
}

/// Match exactly `version`, spaces, `=`, spaces, a quoted strict version, spaces.
fn cargo_version_line(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("version")?.trim_start_matches(' ');
    let rest = rest.strip_prefix('=')?.trim_start_matches(' ');
    let rest = rest.strip_prefix('"')?;
    let (version, tail) = rest.split_once('"')?;
    (tail.trim_start_matches(' ').is_empty() && is_strict_semver(version)).then_some(version)
}

/// Strict `X.Y.Z`: three numeric parts, no leading zeros, no suffixes.
#[must_use]
pub fn is_strict_semver(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

/// Artifact id: lowercase alphanumeric words joined by single hyphens.
#[must_use]
pub fn is_artifact_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_BYTES
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && id.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// One parsed release tag, `<artifact-id>-v<X.Y.Z>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTag {
    /// Declared artifact id.
    pub artifact: String,
    /// Strict `X.Y.Z` version.
    pub version: String,
}

impl ReleaseTag {
    /// Parse a tag name; `None` unless both parts satisfy the strict grammar.
    ///
    /// The split is at the last `-v`; a `SemVer` never contains `-v`, so an id
    /// such as `ui-v1` still parses unambiguously.
    #[must_use]
    pub fn parse(tag: &str) -> Option<Self> {
        let (artifact, version) = tag.rsplit_once("-v")?;
        (is_artifact_id(artifact) && is_strict_semver(version)).then(|| Self {
            artifact: artifact.to_owned(),
            version: version.to_owned(),
        })
    }

    /// Render the tag name.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}-v{}", self.artifact, self.version)
    }
}

/// One release-asset bundle (`[[publish.bundles]]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishBundle {
    /// Artifact id, also the tag prefix (`<id>-v<X.Y.Z>`).
    pub id: String,
    /// Repo-relative files or directories packed into the bundle; sorted, unique.
    pub paths: Vec<String>,
    /// Inline version literal; exclusive with `version_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Manifest holding the version; exclusive with `version`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_file: Option<String>,
}

/// Publication section (`[publish]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishConfig {
    /// Release-asset bundles, sorted by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bundles: Vec<PublishBundle>,
}

impl PublishConfig {
    /// True when nothing is declared (serialization skip predicate).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bundles.is_empty()
    }

    /// Validate every declaration.
    ///
    /// # Errors
    ///
    /// Returns a config error naming the first violated key.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.bundles.len() > MAX_BUNDLES {
            return Err(ContractError::config(file, "publish.bundles", "too_many"));
        }
        let mut seen = BTreeSet::new();
        for (index, bundle) in self.bundles.iter().enumerate() {
            bundle.validate(file, &format!("publish.bundles[{index}]"))?;
            if !seen.insert(bundle.id.as_str()) {
                return Err(ContractError::config(
                    file,
                    "publish.bundles",
                    format!("duplicate_id:{}", bundle.id),
                ));
            }
        }
        let ids: Vec<&str> = self.bundles.iter().map(|b| b.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        if sorted != ids {
            return Err(ContractError::config(
                file,
                "publish.bundles",
                "must_be_sorted_by_id",
            ));
        }
        Ok(())
    }
}

impl PublishBundle {
    /// Validate id, paths and the single version source.
    ///
    /// # Errors
    ///
    /// Returns a config error naming the first violated key.
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        if !is_artifact_id(&self.id) {
            return Err(ContractError::config(
                file,
                format!("{key}.id"),
                format!("bad_artifact_id:{}", self.id),
            ));
        }
        validate_paths(file, &format!("{key}.paths"), &self.paths)?;
        match (&self.version, &self.version_file) {
            (Some(_), Some(_)) => Err(ContractError::config(
                file,
                format!("{key}.version"),
                "version_and_version_file",
            )),
            (None, None) => Err(ContractError::config(
                file,
                format!("{key}.version_file"),
                "missing_version_source",
            )),
            (Some(version), None) if !is_strict_semver(version) => Err(ContractError::config(
                file,
                format!("{key}.version"),
                format!("bad_version:{version}"),
            )),
            (None, Some(path)) => validate_version_file(file, &format!("{key}.version_file"), path),
            (Some(_), None) => Ok(()),
        }
    }

    /// Version source family when a `version_file` is declared.
    #[must_use]
    pub fn version_source(&self) -> Option<VersionSource> {
        self.version_file
            .as_deref()
            .and_then(VersionSource::for_path)
    }
}

/// Validate a version manifest path: safe relative path of a known family.
fn validate_version_file(file: &str, key: &str, path: &str) -> Result<(), ContractError> {
    validate_repo_path(file, key, path)?;
    if VersionSource::for_path(path).is_none() {
        return Err(ContractError::config(
            file,
            key,
            format!("unsupported_version_file:{path}"),
        ));
    }
    Ok(())
}

/// Validate the bundle path list (non-empty, sorted, unique, safe).
fn validate_paths(file: &str, key: &str, paths: &[String]) -> Result<(), ContractError> {
    if paths.is_empty() || paths.len() > MAX_PATHS {
        return Err(ContractError::config(file, key, "bad_path_count"));
    }
    for path in paths {
        validate_repo_path(file, key, path)?;
    }
    let mut sorted = paths.to_vec();
    sorted.sort();
    if sorted != paths {
        return Err(ContractError::config(file, key, "must_be_sorted"));
    }
    let unique: BTreeSet<&str> = paths.iter().map(String::as_str).collect();
    if unique.len() != paths.len() {
        return Err(ContractError::config(file, key, "duplicate_path"));
    }
    Ok(())
}

/// Safe repo-relative path: plain charset, no empty, `.` or `..` segments.
fn validate_repo_path(file: &str, key: &str, path: &str) -> Result<(), ContractError> {
    let plain = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_');
    let segments_ok = path
        .split('/')
        .all(|seg| !seg.is_empty() && seg != "." && seg != ".." && seg != ".git");
    if path.is_empty() || path.len() > 240 || !path.bytes().all(plain) || !segments_ok {
        return Err(ContractError::config(
            file,
            key,
            format!("unsafe_path:{path}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "publish_tests.rs"]
mod tests;
