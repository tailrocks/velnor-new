//! Typed package updater fixture source and artifact mappings.
use super::is_valid_workload_path;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

/// Closed archive encodings used to create package-release fixtures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageUpdateArchive {
    /// Compressed tar archive.
    TarGz,
    /// ZIP archive.
    Zip,
}

/// Generated stable output receiving an artifact's checksum and URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageUpdateOutput {
    /// Stable Homebrew formula.
    Formula,
    /// Stable Homebrew cask.
    Cask,
}

/// Names are data; the generator owns versions, encodings and case operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageUpdateArtifact {
    /// Artifact basename prefix.
    pub prefix: String,
    /// Render-safe platform target identity.
    pub target: String,
    /// Closed fixture archive encoding.
    pub archive: PackageUpdateArchive,
    /// Declared output updated by stable cases.
    pub output: PackageUpdateOutput,
    /// Include this artifact in preview cases.
    pub preview: bool,
    /// Supply the single runnable preview fixture binary.
    pub executable: bool,
}

/// Source updater program is the unit under test, never a task runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageUpdateFixture {
    /// Indexed source shell program under test.
    pub updater: String,
    /// Release source repository identity (`owner/name`).
    pub repository: String,
    /// Indexed stable formula source.
    pub formula: String,
    /// Indexed preview formula source.
    pub preview_formula: String,
    /// Indexed stable cask source, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cask: Option<String>,
    /// Binary basename exercised by preview cases.
    pub binary: String,
    /// Closed artifact fixture mappings.
    pub artifacts: Vec<PackageUpdateArtifact>,
    /// Additional preview JSON artifact identities.
    #[serde(default)]
    pub supporting_manifests: Vec<String>,
}

impl PackageUpdateFixture {
    /// Validate fixture paths, identity mappings, and generated name uniqueness.
    /// # Errors
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        let profile = self;
        let reject = || ContractError::config(file, key, "package_update_invalid_profile");
        if !valid_paths(profile) || !valid_repository(&profile.repository) {
            return Err(reject());
        }
        if !component(&profile.binary)
            || profile.artifacts.is_empty()
            || profile.artifacts.len() > 64
            || profile.supporting_manifests.len() > 16
        {
            return Err(reject());
        }
        let executable = profile
            .artifacts
            .iter()
            .filter(|artifact| artifact.executable);
        if executable.clone().count() != 1
            || executable.into_iter().any(|artifact| {
                !artifact.preview
                    || artifact.archive != PackageUpdateArchive::TarGz
                    || artifact.output != PackageUpdateOutput::Formula
            })
        {
            return Err(reject());
        }
        if profile.artifacts.iter().any(|artifact| {
            !component(&artifact.prefix)
                || !component(&artifact.target)
                || (artifact.output == PackageUpdateOutput::Cask
                    && (profile.cask.is_none() || artifact.preview))
        }) || !profile
            .artifacts
            .iter()
            .any(|artifact| artifact.output == PackageUpdateOutput::Formula)
            || (profile.cask.is_some()
                && !profile
                    .artifacts
                    .iter()
                    .any(|artifact| artifact.output == PackageUpdateOutput::Cask))
        {
            return Err(reject());
        }
        validate_names(profile).map_err(|()| reject())
    }
}

fn valid_paths(profile: &PackageUpdateFixture) -> bool {
    let paths = [&profile.updater, &profile.formula, &profile.preview_formula]
        .into_iter()
        .chain(profile.cask.iter())
        .collect::<Vec<_>>();
    paths.iter().all(|path| is_valid_workload_path(path))
        && paths.iter().collect::<BTreeSet<_>>().len() == paths.len()
        && has_extension(&profile.updater, "sh")
        && [&profile.formula, &profile.preview_formula]
            .iter()
            .all(|path| path.starts_with("Formula/") && has_extension(path, "rb"))
        && profile
            .cask
            .as_ref()
            .is_none_or(|path| path.starts_with("Casks/") && has_extension(path, "rb"))
}

fn has_extension(path: &str, expected: &str) -> bool {
    let path = Path::new(path);
    let file_name = path.file_name().and_then(|name| name.to_str());
    path.extension()
        .is_some_and(|extension| extension == expected)
        || file_name.is_some_and(|name| {
            name.strip_prefix('.')
                .is_some_and(|extension| extension == expected)
        })
}

fn valid_repository(repository: &str) -> bool {
    let Some((owner, name)) = repository.split_once('/') else {
        return false;
    };
    component(owner) && component(name) && owner.len() <= 39 && name.len() <= 100
}

fn component(value: &str) -> bool {
    !value.contains('/') && is_valid_workload_path(value)
}

fn validate_names(profile: &PackageUpdateFixture) -> Result<(), ()> {
    let mut stable = BTreeSet::new();
    let mut preview = BTreeSet::from([
        "release-manifest.json".to_owned(),
        "identity.json".to_owned(),
        "SHA256SUMS".to_owned(),
    ]);
    for artifact in &profile.artifacts {
        let suffix = match artifact.archive {
            PackageUpdateArchive::TarGz => "tar.gz",
            PackageUpdateArchive::Zip => "zip",
        };
        if !stable.insert(format!(
            "{}-1.2.3-{}.{suffix}",
            artifact.prefix, artifact.target
        )) {
            return Err(());
        }
        if artifact.preview {
            let name = format!("{}-{}.{suffix}", artifact.prefix, artifact.target);
            for extra in ["", ".sha256", ".bundle", ".sbom.json"] {
                if !preview.insert(format!("{name}{extra}")) {
                    return Err(());
                }
            }
        }
    }
    for name in &profile.supporting_manifests {
        if !component(name) || !has_extension(name, "json") {
            return Err(());
        }
        for suffix in ["", ".bundle"] {
            if !preview.insert(format!("{name}{suffix}")) {
                return Err(());
            }
        }
    }
    Ok(())
}
