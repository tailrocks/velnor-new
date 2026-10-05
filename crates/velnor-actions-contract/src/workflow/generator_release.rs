//! Stack-neutral version and asset data for a generator release.

use crate::errors::ContractError;
use crate::require_release_version;
use crate::targets::{RELEASE_MANIFEST_FILENAME, SUPPORTED_TARGETS, asset_filename};

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

    /// Rust target triple recorded in the canonical release manifest.
    #[must_use]
    pub const fn triple(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => SUPPORTED_TARGETS[0],
            Self::MacosArm64 => SUPPORTED_TARGETS[1],
        }
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

/// Validated, stack-neutral release version and exact two-target inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorReleasePlan {
    version: String,
}

impl GeneratorReleasePlan {
    /// Build a plan for one exact numeric release version.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] unless `version` is an exact `X.Y.Z` release.
    pub fn for_version(version: &str) -> Result<Self, ContractError> {
        require_release_version(version, RELEASE_MANIFEST_FILENAME)?;
        Ok(Self {
            version: version.to_owned(),
        })
    }

    /// Exact Cargo package release version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Governed immutable release tag for this version.
    #[must_use]
    pub fn tag(&self) -> String {
        format!("v{}", self.version)
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

    /// All five publication assets: each binary and sidecar, then manifest.
    #[must_use]
    pub fn asset_names(&self) -> Vec<String> {
        let mut names = Vec::with_capacity(5);
        for target in self.targets() {
            names.push(target.binary_filename(&self.version));
            names.push(target.sidecar_filename(&self.version));
        }
        names.push(RELEASE_MANIFEST_FILENAME.to_owned());
        names
    }
}

#[cfg(test)]
#[path = "generator_release_tests.rs"]
mod tests;
