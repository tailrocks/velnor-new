//! Stack-neutral source identity, version, and asset data for a generator release.

use crate::errors::ContractError;
use crate::require_release_version;
use crate::targets::{RELEASE_MANIFEST_FILENAME, ReleaseTarget, asset_filename};

const RELEASE_MANIFEST_CHECKSUM_FILENAME: &str = "velnor-actions-release-manifest.json.sha256";

/// One platform-specific generator release target in canonical order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorReleaseTarget {
    /// GNU/Linux x86-64, built on the oldest supported Ubuntu release.
    LinuxX86_64,
    /// macOS ARM64, built on a native Apple Silicon runner.
    MacosArm64,
}

impl GeneratorReleaseTarget {
    /// Every supported target, in release-manifest order.
    pub const ALL: [Self; 2] = [Self::LinuxX86_64, Self::MacosArm64];

    /// Canonical release target represented by this producer target.
    #[must_use]
    pub const fn release_target(self) -> ReleaseTarget {
        match self {
            Self::LinuxX86_64 => ReleaseTarget::LinuxX86_64,
            Self::MacosArm64 => ReleaseTarget::MacosArm64,
        }
    }

    /// Rust target triple recorded in the canonical release manifest.
    #[must_use]
    pub const fn triple(self) -> &'static str {
        self.release_target().triple()
    }

    /// Native runner selector used to build this target.
    #[must_use]
    pub const fn runner_label(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "ubuntu-22.04",
            Self::MacosArm64 => "macos-15",
        }
    }

    /// Versioned binary asset filename.
    #[must_use]
    pub fn binary_filename(self, version: &str) -> String {
        asset_filename(version, self.triple())
    }

    /// SHA-256 sidecar filename for the binary asset.
    #[must_use]
    pub fn sidecar_filename(self, version: &str) -> String {
        format!("{}.sha256", self.binary_filename(version))
    }
}

/// Source selection for a release workflow generated before its dispatch commit exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneratorReleaseSource {
    CurrentWorkflow,
}

/// Validated release version bound to the current workflow source at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorReleaseSourceBinding {
    version: String,
    source: GeneratorReleaseSource,
}

impl GeneratorReleaseSourceBinding {
    /// Bind the release version to the workflow's dispatch commit.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] unless `version` is stable `X.Y.Z` SemVer.
    pub fn for_current_workflow(version: &str) -> Result<Self, ContractError> {
        require_semver_release_version(version)?;
        Ok(Self {
            version: version.to_owned(),
            source: GeneratorReleaseSource::CurrentWorkflow,
        })
    }

    /// Exact Cargo package release version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Exact two-target inventory, in canonical manifest order.
    #[must_use]
    pub const fn targets(&self) -> [GeneratorReleaseTarget; 2] {
        GeneratorReleaseTarget::ALL
    }

    /// Four staged binary and checksum assets, in canonical target order.
    #[must_use]
    pub fn staged_asset_names(&self) -> Vec<String> {
        staged_asset_names(&self.version)
    }

    /// Six final assets: staged binaries and checksums, then manifest files.
    #[must_use]
    pub fn final_asset_names(&self) -> Vec<String> {
        final_asset_names(&self.version)
    }

    /// GitHub expression for the typed current-workflow source binding.
    #[must_use]
    pub const fn source_expression(&self) -> &'static str {
        match self.source {
            GeneratorReleaseSource::CurrentWorkflow => "${{ github.sha }}",
        }
    }

    /// Resolve a runtime source commit into an exact release plan.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] unless `source_sha` is exactly 40 lowercase
    /// hexadecimal characters.
    pub fn bind(&self, source_sha: &str) -> Result<GeneratorReleasePlan, ContractError> {
        GeneratorReleasePlan::for_version_and_source(&self.version, source_sha)
    }
}

/// Validated source identity, release version, and canonical asset inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorReleasePlan {
    version: String,
    source_sha: String,
}

impl GeneratorReleasePlan {
    /// Build a plan for one exact SemVer release and one source commit.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] unless `version` is stable `X.Y.Z` SemVer and
    /// `source_sha` contains exactly 40 lowercase hexadecimal characters.
    pub fn for_version_and_source(version: &str, source_sha: &str) -> Result<Self, ContractError> {
        require_semver_release_version(version)?;
        if !crate::ids::is_lower_hex_len(source_sha, 40) {
            return Err(ContractError::config(
                RELEASE_MANIFEST_FILENAME,
                "source_sha",
                "malformed_source_commit",
            ));
        }
        Ok(Self {
            version: version.to_owned(),
            source_sha: source_sha.to_owned(),
        })
    }

    /// Exact Cargo package release version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Exact lowercase source commit identity.
    #[must_use]
    pub fn source_sha(&self) -> &str {
        &self.source_sha
    }

    /// Canonical immutable source-bound release tag.
    #[must_use]
    pub fn tag(&self) -> String {
        format!("generator-{}", self.source_sha)
    }

    /// Canonical owner/repository identity.
    #[must_use]
    pub const fn repository(&self) -> &'static str {
        crate::targets::EXPECTED_REPOSITORY
    }

    /// Exact two-target inventory, in canonical manifest order.
    #[must_use]
    pub const fn targets(&self) -> [GeneratorReleaseTarget; 2] {
        GeneratorReleaseTarget::ALL
    }

    /// Four staged binary and checksum assets, in canonical target order.
    #[must_use]
    pub fn staged_asset_names(&self) -> Vec<String> {
        staged_asset_names(&self.version)
    }

    /// Six final assets: staged binaries and checksums, then manifest files.
    #[must_use]
    pub fn final_asset_names(&self) -> Vec<String> {
        final_asset_names(&self.version)
    }
}

fn staged_asset_names(version: &str) -> Vec<String> {
    let mut names = Vec::with_capacity(4);
    for target in GeneratorReleaseTarget::ALL {
        names.push(target.binary_filename(version));
        names.push(target.sidecar_filename(version));
    }
    names
}

fn final_asset_names(version: &str) -> Vec<String> {
    let mut names = staged_asset_names(version);
    names.push(RELEASE_MANIFEST_FILENAME.to_owned());
    names.push(RELEASE_MANIFEST_CHECKSUM_FILENAME.to_owned());
    names
}

fn require_semver_release_version(version: &str) -> Result<(), ContractError> {
    require_release_version(version, RELEASE_MANIFEST_FILENAME)?;
    if has_semver_release_core(version) {
        Ok(())
    } else {
        Err(ContractError::config(
            RELEASE_MANIFEST_FILENAME,
            "version",
            "non_semver_release",
        ))
    }
}

fn has_semver_release_core(version: &str) -> bool {
    version
        .split('.')
        .all(|part| (part == "0" || !part.starts_with('0')) && part.parse::<u64>().is_ok())
}

#[cfg(test)]
#[path = "generator_release_tests.rs"]
mod tests;
