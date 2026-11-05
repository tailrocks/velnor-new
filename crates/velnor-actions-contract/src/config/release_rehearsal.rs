// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::release_validation::{validate_environment, validate_manifest_path};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
    pub qualification: ReleaseQualificationPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseQualificationPolicy {
    pub candidate_repository: String,
    pub candidate_repository_id: u64,
    pub candidate_default_branch: String,
    pub candidate_required_workflow_path: String,
    pub completion_app_id: u64,
    pub completion_check_name: String,
    pub reviewer_app_ids: BTreeSet<u64>,
    pub historical_manifest: ReleaseHistoricalManifestPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseHistoricalManifestPolicy {
    pub version: String,
    pub source_sha: String,
    pub sha256: String,
}

fn validate_qualification_policy(
    file: &str,
    policy: &ReleaseQualificationPolicy,
) -> Result<(), ContractError> {
    let workflow = policy
        .candidate_required_workflow_path
        .strip_prefix(".github/workflows/");
    let workflow_valid = workflow.is_some_and(|name| {
        !name.is_empty()
            && !name.contains('/')
            && std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml")
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    });
    if !crate::vcs::is_safe_github_repository(&policy.candidate_repository)
        || policy.candidate_repository_id == 0
        || !crate::is_valid_branch_name(&policy.candidate_default_branch)
        || !workflow_valid
        || policy.completion_app_id == 0
        || policy.completion_check_name.is_empty()
        || policy.completion_check_name.len() > 200
        || policy.completion_check_name.chars().any(char::is_control)
        || policy.completion_check_name.contains("${{")
        || policy.reviewer_app_ids.is_empty()
        || policy.reviewer_app_ids.len() > 32
        || policy.reviewer_app_ids.contains(&0)
    {
        return Err(ContractError::config(
            file,
            "stacks.rust.release.rehearsal.metadata.qualification",
            "invalid_qualification_policy",
        ));
    }
    let historical = &policy.historical_manifest;
    if !super::super::release_intent::is_cargo_version(&historical.version)
        || !crate::ids::is_lower_hex_len(&historical.source_sha, 40)
        || !crate::ids::is_lower_hex_len(&historical.sha256, 64)
    {
        return Err(ContractError::config(
            file,
            "stacks.rust.release.rehearsal.metadata.historical_manifest",
            "invalid_historical_manifest",
        ));
    }
    Ok(())
}

impl ReleaseRehearsalConfig {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        validate_environment(file, &self.environment)?;
        if let Some(metadata) = &self.metadata {
            validate_manifest_path(file, &metadata.manifest_path)?;
            validate_qualification_policy(file, &metadata.qualification)?;
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
