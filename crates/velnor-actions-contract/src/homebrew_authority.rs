//! Data-only locator for an authenticated source-owned Homebrew policy.

use serde::{Deserialize, Serialize};

use crate::{ContractError, canonical_json_bytes, strict_json::parse_strict_json_with_limit};

/// Closed schema identifier for tap-base Homebrew source locators.
pub const HOMEBREW_SOURCE_AUTHORITY_LOCATOR_SCHEMA_V1: &str = "velnor.homebrew-source-authority.v1";
/// Exact tap-base path containing the protected locator.
pub const HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH: &str = ".velnor/homebrew-source-authority.json";
/// Exact source config path admitted by the locator.
pub const HOMEBREW_SOURCE_CONFIG_PATH: &str = ".velnor/config.toml";
/// Exact static package release policy path admitted by the locator.
pub const HOMEBREW_SOURCE_RELEASE_POLICY_PATH: &str = ".velnor/homebrew-release-policy.json";
/// Maximum locator size.
pub const MAX_HOMEBREW_SOURCE_AUTHORITY_LOCATOR_BYTES: usize = 16 * 1024;

/// Data-only source identity and policy paths read from an authenticated tap base.
///
/// This record grants no execution permission. Runtime code must first
/// authenticate its exact tap-base Git blob, then independently verify the
/// named source repository and read its protected default-branch tip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewSourceAuthorityLocator {
    /// Closed schema discriminator.
    pub schema: String,
    /// Registered source repository slug.
    pub repository: String,
    /// GitHub numeric repository identity.
    pub repository_id: u64,
    /// Protected source default branch.
    pub default_branch: String,
    /// Exact protected source config path.
    pub config_path: String,
    /// Exact protected package release policy path.
    pub release_policy_path: String,
}

impl HomebrewSourceAuthorityLocator {
    /// Validate the locator's fixed schema and repository/path identities.
    ///
    /// # Errors
    /// Returns an error for an invalid source repository, branch, or fixed policy path.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != HOMEBREW_SOURCE_AUTHORITY_LOCATOR_SCHEMA_V1
            || !crate::vcs::is_safe_github_repository(&self.repository)
            || self.repository_id == 0
            || !crate::is_valid_branch_name(&self.default_branch)
            || self.config_path != HOMEBREW_SOURCE_CONFIG_PATH
            || self.release_policy_path != HOMEBREW_SOURCE_RELEASE_POLICY_PATH
        {
            return Err(ContractError::CanonicalJson(
                "invalid_homebrew_source_authority_locator".to_owned(),
            ));
        }
        Ok(())
    }

    /// Validate and serialize the canonical locator bytes.
    ///
    /// # Errors
    /// Returns an error when locator validation or canonical serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        canonical_json_bytes(self)
    }
}

/// Parse a bounded, strict JSON source locator.
///
/// Duplicate keys, unknown fields, invalid UTF-8, oversized documents, and
/// noncanonical serialization are rejected before the result can be used.
///
/// # Errors
/// Returns an error for malformed, oversized, invalid, or noncanonical locator bytes.
pub fn parse_homebrew_source_authority_locator(
    bytes: &[u8],
) -> Result<HomebrewSourceAuthorityLocator, ContractError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| ContractError::CanonicalJson(error.to_string()))?;
    let value = parse_strict_json_with_limit(text, MAX_HOMEBREW_SOURCE_AUTHORITY_LOCATOR_BYTES)?;
    let locator: HomebrewSourceAuthorityLocator = serde_json::from_value(value)
        .map_err(|error| ContractError::CanonicalJson(error.to_string()))?;
    locator.validate()?;
    if locator.canonical_bytes()? != bytes {
        return Err(ContractError::CanonicalJson(
            "homebrew_source_authority_locator_not_canonical".to_owned(),
        ));
    }
    Ok(locator)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> HomebrewSourceAuthorityLocator {
        HomebrewSourceAuthorityLocator {
            schema: HOMEBREW_SOURCE_AUTHORITY_LOCATOR_SCHEMA_V1.to_owned(),
            repository: "jackin-project/jackin".to_owned(),
            repository_id: 42,
            default_branch: "main".to_owned(),
            config_path: HOMEBREW_SOURCE_CONFIG_PATH.to_owned(),
            release_policy_path: HOMEBREW_SOURCE_RELEASE_POLICY_PATH.to_owned(),
        }
    }

    #[test]
    fn parses_only_canonical_closed_locator() {
        let locator = valid();
        let bytes = locator.canonical_bytes().expect("canonical");
        assert_eq!(parse_homebrew_source_authority_locator(&bytes), Ok(locator));
        assert!(parse_homebrew_source_authority_locator(
            br#"{"schema":"velnor.homebrew-source-authority.v1","schema":"velnor.homebrew-source-authority.v1"}"#
        )
        .is_err());
        assert!(
            parse_homebrew_source_authority_locator(
                br#"{"schema":"velnor.homebrew-source-authority.v1","unexpected":true}"#
            )
            .is_err()
        );
    }

    #[test]
    fn locator_paths_and_source_identity_are_fixed_and_validated() {
        let mut locator = valid();
        locator.config_path = "candidate.toml".to_owned();
        assert!(locator.validate().is_err());
        locator = valid();
        locator.repository_id = 0;
        assert!(locator.validate().is_err());
        locator = valid();
        locator.repository = "https://github.com/jackin-project/jackin".to_owned();
        assert!(locator.validate().is_err());
    }
}
