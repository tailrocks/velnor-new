// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::release_validation::{validate_environment, validate_manifest_path};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRehearsalConfig {
    pub script: String,
    pub environment: String,
    pub reviewer_id: u64,
    #[serde(default)]
    pub output_bundle: bool,
    #[serde(default)]
    pub metadata: Option<ReleaseRehearsalMetadataConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRehearsalMetadataConfig {
    pub manifest_path: String,
    pub rust_version: String,
    pub intent_config_path: String,
}

impl ReleaseRehearsalConfig {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        validate_environment(file, &self.environment)?;
        if let Some(metadata) = &self.metadata {
            validate_manifest_path(file, &metadata.manifest_path)?;
            let path = &metadata.intent_config_path;
            if path.is_empty()
                || path.starts_with('/')
                || !std::path::Path::new(path)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
                || path
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
                || !path
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'.'))
            {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.release.rehearsal.metadata.intent_config_path",
                    "invalid_intent_config_path",
                ));
            }
            if !super::super::release_intent::is_cargo_version(&metadata.rust_version) {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.release.rehearsal.metadata.rust_version",
                    "invalid_rust_version",
                ));
            }
        }
        let path = &self.script;
        if self.reviewer_id == 0
            || !self
                .environment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            || path.is_empty()
            || path.starts_with('/')
            || !std::path::Path::new(path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("sh"))
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || !path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'.'))
        {
            return Err(ContractError::config(
                file,
                "stacks.rust.release.rehearsal",
                "invalid_rehearsal_config",
            ));
        }
        Ok(())
    }
}
