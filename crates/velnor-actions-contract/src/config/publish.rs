use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const RELEASE_TAG_GLOB: &str = "*-v[0-9]*.[0-9]*.[0-9]*";

const MAX_ID_BYTES: usize = 64;

pub(super) const MAX_BUNDLES: usize = 64;

const MAX_PATHS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionSource {
    CargoToml,

    GradleProperties,

    PackageJson,
}

impl VersionSource {
    #[must_use]
    pub fn for_path(path: &str) -> Option<Self> {
        match path.rsplit('/').next()? {
            "Cargo.toml" => Some(Self::CargoToml),
            "gradle.properties" => Some(Self::GradleProperties),
            "package.json" => Some(Self::PackageJson),
            _ => None,
        }
    }

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

fn cargo_version_line(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("version")?.trim_start_matches(' ');
    let rest = rest.strip_prefix('=')?.trim_start_matches(' ');
    let rest = rest.strip_prefix('"')?;
    let (version, tail) = rest.split_once('"')?;
    (tail.trim_start_matches(' ').is_empty() && is_strict_semver(version)).then_some(version)
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTag {
    pub artifact: String,

    pub version: String,
}

impl ReleaseTag {
    #[must_use]
    pub fn parse(tag: &str) -> Option<Self> {
        let (artifact, version) = tag.rsplit_once("-v")?;
        (is_artifact_id(artifact) && is_strict_semver(version)).then(|| Self {
            artifact: artifact.to_owned(),
            version: version.to_owned(),
        })
    }

    #[must_use]
    pub fn render(&self) -> String {
        format!("{}-v{}", self.artifact, self.version)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishBundle {
    pub id: String,

    pub paths: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_file: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishProvenance {
    #[default]
    GithubAttestation,

    SigstoreBundle,
}

impl PublishProvenance {
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::GithubAttestation
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishConfig {
    #[serde(default, skip_serializing_if = "PublishProvenance::is_default")]
    pub provenance: PublishProvenance,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bundles: Vec<PublishBundle>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub npm: Vec<super::publish_npm::PublishNpm>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maven: Vec<super::publish_maven::PublishMaven>,
}

impl PublishConfig {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bundles.is_empty() && self.npm.is_empty() && self.maven.is_empty()
    }

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
        self.validate_npm(file, &mut seen)?;
        super::publish_maven::validate_entries(&self.maven, file, &mut seen)
    }
}

impl PublishConfig {
    fn validate_npm<'a>(
        &'a self,
        file: &str,
        taken: &mut BTreeSet<&'a str>,
    ) -> Result<(), ContractError> {
        if self.npm.len() > MAX_BUNDLES {
            return Err(ContractError::config(file, "publish.npm", "too_many"));
        }
        for (index, entry) in self.npm.iter().enumerate() {
            entry.validate(file, &format!("publish.npm[{index}]"))?;
            if !taken.insert(entry.id.as_str()) {
                return Err(ContractError::config(
                    file,
                    "publish.npm",
                    format!("duplicate_id:{}", entry.id),
                ));
            }
        }
        if self.npm.windows(2).any(|pair| pair[0].id >= pair[1].id) {
            return Err(ContractError::config(
                file,
                "publish.npm",
                "must_be_sorted_by_id",
            ));
        }
        Ok(())
    }
}

impl PublishBundle {
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

    #[must_use]
    pub fn version_source(&self) -> Option<VersionSource> {
        self.version_file
            .as_deref()
            .and_then(VersionSource::for_path)
    }
}

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

pub(super) fn validate_repo_path(file: &str, key: &str, path: &str) -> Result<(), ContractError> {
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
