// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use super::release_validation::{
    is_full_sha, is_package_name, validate_environment, validate_manifest_path, validate_packages,
    validate_publish_features, validate_tag_name, validate_version_groups,
};
use crate::errors::ContractError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateReleaseIntent {
    pub schema: u32,
    pub repository: String,
    pub default_branch: String,
    pub environment: String,
    pub required_workflow_path: String,
    pub trusted_reviewer_id: u64,
    pub rust_version: String,
    pub generator_version: String,
    pub generator_source_sha: String,
    pub generator_manifest_sha256: String,
    pub release_plz_version: String,
    pub runner: String,
    pub authentication: super::ReleaseAuthentication,
    pub manifest_path: String,
    pub pr_write_paths: Vec<String>,
    pub registry: String,
    pub packages: Vec<String>,
    pub publish_features: std::collections::BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub version_groups: std::collections::BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_crates: Option<super::NewCrateRelease>,
    pub tag_template: String,
    pub recovery: Option<CrateReleaseRecovery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub journal_predecessor: Option<super::CrateReleasePredecessor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_source_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateReleaseRecovery {
    pub package: String,
    pub version: String,
    pub source_sha: String,
}

impl CrateReleaseIntent {
    #[must_use]
    pub fn exact_source_sha(&self) -> Option<&str> {
        self.recovery
            .as_ref()
            .map(|item| item.source_sha.as_str())
            .or_else(|| {
                self.new_crates
                    .as_ref()
                    .map(|item| item.source_sha.as_str())
            })
            .or(self.continuation_source_sha.as_deref())
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        let file = "release_intent";
        let invalid = |key, problem| ContractError::config(file, key, problem);
        if self.schema != 1 {
            return Err(invalid("schema", "unsupported_schema"));
        }
        let mut parts = self.repository.split('/');
        let owner = parts.next().unwrap_or_default();
        let name = parts.next().unwrap_or_default();
        if parts.next().is_some() || !repository_part(owner) || !repository_part(name) {
            return Err(invalid("repository", "invalid_repository"));
        }
        if !branch(&self.default_branch) {
            return Err(invalid("default_branch", "invalid_branch"));
        }
        if self.registry != "crates-io" {
            return Err(invalid("registry", "unsupported_registry"));
        }
        validate_environment(file, &self.environment)?;
        if !is_full_sha(&self.generator_source_sha)
            || self.generator_manifest_sha256.len() != 64
            || !self
                .generator_manifest_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !super::is_exact_semver(&self.generator_version)
            || !super::is_exact_semver(&self.release_plz_version)
            || self.runner != "ubuntu-26.04"
        {
            return Err(invalid("tool_expectations", "invalid_tool_expectations"));
        }
        if !super::is_exact_semver(&self.rust_version) {
            return Err(invalid("rust_version", "invalid_rust_version"));
        }
        if self.trusted_reviewer_id == 0 {
            return Err(invalid("trusted_reviewer_id", "invalid_reviewer"));
        }
        if !super::release_validation::valid_release_workflow_path(&self.required_workflow_path) {
            return Err(invalid("required_workflow_path", "invalid_workflow_path"));
        }
        validate_manifest_path(file, &self.manifest_path)?;
        validate_pr_write_paths(self)?;
        validate_intent_packages(self)?;
        super::release_validation::validate_journal_predecessor(
            file,
            self.journal_predecessor.as_ref(),
            self.exact_source_sha().is_some(),
        )?;
        super::release_validation::validate_continuation_source(
            file,
            self.continuation_source_sha.as_deref(),
            self.journal_predecessor.is_some(),
        )?;
        super::release_validation::validate_exact_source_agreement(
            file,
            self.recovery.as_ref().map(|r| r.source_sha.as_str()),
            self.new_crates.as_ref().map(|r| r.source_sha.as_str()),
            self.continuation_source_sha.as_deref(),
        )?;
        validate_tag_name(file, &self.tag_template)?;
        if !super::release_validation::supported_crate_tag_template(&self.tag_template) {
            return Err(invalid("tag_template", "unsupported_tag_template"));
        }
        if let Some(recovery) = &self.recovery
            && (self.packages.len() != 1
                || self.packages[0] != recovery.package
                || !is_package_name(&recovery.package)
                || recovery.version.len() > 150
                || !is_cargo_version(&recovery.version)
                || !is_full_sha(&recovery.source_sha))
        {
            return Err(invalid("recovery", "invalid_recovery"));
        }
        Ok(())
    }
}

fn validate_pr_write_paths(intent: &CrateReleaseIntent) -> Result<(), ContractError> {
    if intent.pr_write_paths.is_empty()
        || intent.pr_write_paths.len() > 256
        || !intent
            .pr_write_paths
            .iter()
            .any(|path| path == &intent.manifest_path)
        || intent
            .pr_write_paths
            .windows(2)
            .any(|paths| paths[0] >= paths[1])
        || intent.pr_write_paths.iter().any(|path| {
            let name = path
                .rsplit_once('/')
                .map_or(path.as_str(), |(_, name)| name);
            path.len() > 1024
                || !matches!(name, "Cargo.toml" | "Cargo.lock" | "CHANGELOG.md")
                || path.starts_with('/')
                || path
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == ".." || part == ".git")
                || path.bytes().any(|b| b.is_ascii_control() || b == b'\\')
        })
    {
        return Err(ContractError::config(
            "release_intent",
            "pr_write_paths",
            "invalid_pr_write_paths",
        ));
    }
    Ok(())
}

fn validate_intent_packages(intent: &CrateReleaseIntent) -> Result<(), ContractError> {
    let file = "release_intent";
    validate_packages(file, &intent.packages, true, false)?;
    validate_publish_features(file, &intent.publish_features, &intent.packages, false)?;
    validate_version_groups(file, &intent.version_groups, &intent.packages, false)?;
    super::release::validate_new_crates(file, intent.new_crates.as_ref(), &intent.packages, false)?;
    if intent.new_crates.is_some()
        && intent.authentication != super::ReleaseAuthentication::BootstrapToken
    {
        return Err(ContractError::config(
            file,
            "new_crates",
            "bootstrap_token_required",
        ));
    }
    if intent.new_crates.is_some() && intent.recovery.is_some() {
        return Err(ContractError::config(
            file,
            "new_crates",
            "conflicting_release_intents",
        ));
    }
    if intent.publish_features.len() != intent.packages.len() {
        return Err(ContractError::config(
            file,
            "publish_features",
            "package_mapping_mismatch",
        ));
    }
    Ok(())
}

#[must_use]
pub fn is_cargo_version(version: &str) -> bool {
    semver::Version::parse(version).is_ok_and(|parsed| parsed.to_string() == version)
}

fn repository_part(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn branch(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.contains("..")
        && !name.contains("//")
        && !name.ends_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
}
