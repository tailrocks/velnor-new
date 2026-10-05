//! Stack-neutral version and asset data for a generator release.

use crate::errors::ContractError;
use crate::require_release_version;
use crate::targets::{RELEASE_MANIFEST_FILENAME, ReleaseTarget};

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
    pub const fn targets(&self) -> [ReleaseTarget; 2] {
        ReleaseTarget::ALL
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
