// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use super::release_validation::{
    is_full_sha, is_package_name, validate_environment, validate_manifest_path, validate_packages,
    validate_publish_features, validate_tag_name,
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
    pub registry: String,
    pub packages: Vec<String>,
    pub publish_features: std::collections::BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_crates: Option<super::NewCrateRelease>,
    pub tag_template: String,
    pub recovery: Option<CrateReleaseRecovery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub journal_predecessor: Option<super::CrateReleasePredecessor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateReleaseRecovery {
    pub package: String,
    pub version: String,
    pub source_sha: String,
}

impl CrateReleaseIntent {
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
            || self.runner.is_empty()
            || self.runner.len() > 64
            || !self
                .runner
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.'))
        {
            return Err(invalid("tool_expectations", "invalid_tool_expectations"));
        }
        if !super::is_exact_semver(&self.rust_version) {
            return Err(invalid("rust_version", "invalid_rust_version"));
        }
        if self.trusted_reviewer_id == 0 {
            return Err(invalid("trusted_reviewer_id", "invalid_reviewer"));
        }
        let workflow = self
            .required_workflow_path
            .strip_prefix(".github/workflows/");
        if !workflow.is_some_and(|name| {
            !name.starts_with('.')
                && std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext == "yml" || ext == "yaml")
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        }) {
            return Err(invalid("required_workflow_path", "invalid_workflow_path"));
        }
        validate_manifest_path(file, &self.manifest_path)?;
        validate_intent_packages(self)?;
        super::release::validate_journal_predecessor(file, self.journal_predecessor.as_ref())?;
        validate_tag_name(file, &self.tag_template)?;
        let compact: String = self
            .tag_template
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if compact != "{{package}}-v{{version}}" {
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

fn validate_intent_packages(intent: &CrateReleaseIntent) -> Result<(), ContractError> {
    let file = "release_intent";
    validate_packages(file, &intent.packages, true, false)?;
    validate_publish_features(file, &intent.publish_features, &intent.packages, false)?;
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
