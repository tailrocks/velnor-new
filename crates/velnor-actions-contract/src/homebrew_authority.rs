// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::{ContractError, canonical_json_bytes, strict_json::parse_strict_json_with_limit};

pub const HOMEBREW_SOURCE_AUTHORITY_LOCATOR_SCHEMA_V1: &str = "velnor.homebrew-source-authority.v1";

pub const HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH: &str = ".velnor/homebrew-source-authority.json";

pub const HOMEBREW_SOURCE_CONFIG_PATH: &str = ".velnor/config.toml";

pub const HOMEBREW_SOURCE_RELEASE_POLICY_PATH: &str = ".velnor/homebrew-release-policy.json";

pub const MAX_HOMEBREW_SOURCE_AUTHORITY_LOCATOR_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewSourceAuthorityLocator {
    pub schema: String,

    pub repository: String,

    pub repository_id: u64,

    pub default_branch: String,

    pub config_path: String,

    pub release_policy_path: String,
}

impl HomebrewSourceAuthorityLocator {
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

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        canonical_json_bytes(self)
    }
}

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
