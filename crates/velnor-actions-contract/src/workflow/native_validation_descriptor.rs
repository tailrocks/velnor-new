//! Closed semantic requests for source-owned native validation programs.

use crate::{ContractError, config::PackageUpdateFixture};
use serde::{Deserialize, Serialize};

/// Semantic data only; SDK reconstruction supplies source and execution authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum NativeValidationDescriptor {
    /// Immutable package updater fixture harness and typed source mappings.
    PackageUpdateFixture {
        /// Validated source and artifact mapping.
        profile: PackageUpdateFixture,
    },
    /// Immutable bootstrap of the reviewed Homebrew source and local tap.
    HomebrewPreparation {
        /// Canonical lowercase `owner/homebrew-name` repository identity.
        repository: String,
        /// Closed source scope requiring the host cask capability.
        has_casks: bool,
    },
}

impl NativeValidationDescriptor {
    /// Validate semantic shape before the SDK reconstructs its compiled owner.
    /// # Errors
    /// Rejects invalid fixture profiles or noncanonical tap identities.
    pub fn validate(&self) -> Result<(), ContractError> {
        match self {
            Self::PackageUpdateFixture { profile } => {
                profile.validate("native_validation", "package_update_fixture")
            }
            Self::HomebrewPreparation { repository, .. } => {
                let Some((owner, repository)) = repository.split_once('/') else {
                    return Err(invalid());
                };
                let Some(name) = repository.strip_prefix("homebrew-") else {
                    return Err(invalid());
                };
                if owner.len() > 39 || name.len() > 100 || !safe(owner) || !safe(name) {
                    return Err(invalid());
                }
                Ok(())
            }
        }
    }
}

fn safe(value: &str) -> bool {
    let boundary = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    !value.is_empty()
        && value.bytes().next().is_some_and(boundary)
        && value.bytes().last().is_some_and(boundary)
        && value
            .bytes()
            .all(|byte| boundary(byte) || matches!(byte, b'-' | b'_'))
}

fn invalid() -> ContractError {
    ContractError::identity("native_validation", "noncanonical_homebrew_repository")
}
