//! Host reference for the release-reader credential.

use serde::{Deserialize, Serialize};

use crate::error::HostError;

use super::KeychainReference;

/// Keychain reference for release metadata and attestation access.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReleaseReadConfig {
    credential_ref: KeychainReference,
}

impl ReleaseReadConfig {
    pub(super) const fn new(credential_ref: KeychainReference) -> Self {
        Self { credential_ref }
    }

    /// Return the release-reader Keychain reference.
    #[must_use]
    pub(super) const fn credential_ref(&self) -> &KeychainReference {
        &self.credential_ref
    }
}

/// Reject a release-reader reference that reuses a repository credential.
///
/// # Errors
///
/// Returns [`HostError::Config`] when both settings use one Keychain item.
pub(super) fn validate_release_read<'a>(
    release_read: Option<&ReleaseReadConfig>,
    mut connection_refs: impl Iterator<Item = &'a KeychainReference>,
) -> Result<(), HostError> {
    let Some(release_read) = release_read else {
        return Ok(());
    };
    if connection_refs.any(|reference| reference == &release_read.credential_ref) {
        return Err(HostError::Config);
    }
    Ok(())
}

#[cfg(test)]
#[path = "release_read_tests.rs"]
mod tests;
