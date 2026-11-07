// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::release_validation::{
    validate_continuation_source, validate_environment, validate_exact_source_agreement,
    validate_journal_predecessor, validate_manifest_path, validate_packages,
    validate_publish_features, validate_recovery, validate_tag_name, validate_version_groups,
};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_CRATE_RELEASE_PACKAGES: usize = 64;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseAuthentication {
    #[default]
    TrustedPublishing,

    BootstrapToken,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseRegistry {
    CratesIo,

    #[default]
    GitTag,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRelease {
    pub package: String,

    pub version: String,

    pub source_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewCrateRelease {
    pub source_sha: String,
    pub packages: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateReleasePredecessor {
    pub run_id: u64,
    pub run_attempt: u32,
    pub workflow_sha: String,
}

#[path = "release_rehearsal.rs"]
mod rehearsal;
pub use rehearsal::{
    ReleaseHistoricalManifestPolicy, ReleaseQualificationPolicy, ReleaseRehearsalConfig,
    ReleaseRehearsalMetadataConfig, ReleaseReviewProof,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustReleaseConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_manifest_path")]
    pub manifest_path: String,

    #[serde(default)]
    pub packages: Vec<String>,

    #[serde(default)]
    pub publishable_workspace: bool,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub publish_features: BTreeMap<String, Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_crates: Option<NewCrateRelease>,

    #[serde(default = "default_environment")]
    pub environment: String,

    #[serde(default)]
    pub required_workflow_path: String,

    #[serde(default)]
    pub trusted_reviewer_id: u64,

    #[serde(default, skip_serializing_if = "is_default_registry")]
    pub registry: ReleaseRegistry,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authentication: Option<ReleaseAuthentication>,

    #[serde(default = "default_true")]
    pub release_pr: bool,

    #[serde(default = "default_tag_name")]
    pub tag_name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<RecoveryRelease>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub journal_predecessor: Option<CrateReleasePredecessor>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_source_sha: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rehearsal: Option<ReleaseRehearsalConfig>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub version_groups: BTreeMap<String, Vec<String>>,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip predicates take a reference"
)]
fn is_default_registry(registry: &ReleaseRegistry) -> bool {
    *registry == ReleaseRegistry::default()
}

fn default_manifest_path() -> String {
    "Cargo.toml".to_owned()
}

fn default_environment() -> String {
    "release".to_owned()
}

fn default_true() -> bool {
    true
}

fn default_tag_name() -> String {
    "{{ package }}-v{{ version }}".to_owned()
}

impl Default for RustReleaseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            manifest_path: default_manifest_path(),
            packages: Vec::new(),
            publishable_workspace: false,
            publish_features: BTreeMap::new(),
            new_crates: None,
            environment: default_environment(),
            required_workflow_path: String::new(),
            trusted_reviewer_id: 0,
            registry: ReleaseRegistry::default(),
            authentication: None,
            release_pr: true,
            tag_name: default_tag_name(),
            recovery: None,
            journal_predecessor: None,
            continuation_source_sha: None,
            rehearsal: None,
            version_groups: BTreeMap::new(),
        }
    }
}

impl RustReleaseConfig {
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

    #[must_use]
    pub fn effective_authentication(&self) -> ReleaseAuthentication {
        self.authentication.unwrap_or_default()
    }

    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        validate_manifest_path(file, &self.manifest_path)?;
        validate_packages(
            file,
            &self.packages,
            self.enabled,
            self.publishable_workspace,
        )?;
        if self.registry == ReleaseRegistry::CratesIo
            && self.packages.len() > MAX_CRATE_RELEASE_PACKAGES
        {
            return Err(ContractError::config(
                file,
                "stacks.rust.release.packages",
                "too_many_packages",
            ));
        }
        validate_publish_features(
            file,
            &self.publish_features,
            &self.packages,
            self.publishable_workspace,
        )?;
        validate_new_crates(
            file,
            self.new_crates.as_ref(),
            &self.packages,
            self.publishable_workspace,
        )?;
        if self.new_crates.is_some()
            && (self.registry != ReleaseRegistry::CratesIo
                || self.effective_authentication() != ReleaseAuthentication::BootstrapToken)
        {
            return Err(ContractError::config(
                file,
                "new_crates",
                "bootstrap_token_required",
            ));
        }
        validate_environment(file, &self.environment)?;
        if let Some(rehearsal) = &self.rehearsal {
            rehearsal.validate(file)?;
        }
        self.validate_authentication(file)?;
        validate_tag_name(file, &self.tag_name)?;
        if self.enabled && self.registry == ReleaseRegistry::CratesIo {
            if self.trusted_reviewer_id == 0
                || !super::release_validation::valid_release_workflow_path(
                    &self.required_workflow_path,
                )
            {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.release",
                    "invalid_release_admission",
                ));
            }
            if !super::release_validation::supported_crate_tag_template(&self.tag_name) {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.release.tag_name",
                    "unsupported_tag_template",
                ));
            }
        }
        self.validate_recovery_selection(file)?;
        self.validate_sources(file)?;
        validate_version_groups(
            file,
            &self.version_groups,
            &self.packages,
            self.publishable_workspace,
        )?;
        Ok(())
    }

    fn validate_sources(&self, file: &str) -> Result<(), ContractError> {
        validate_journal_predecessor(
            file,
            self.journal_predecessor.as_ref(),
            self.exact_source_sha().is_some(),
        )?;
        validate_continuation_source(
            file,
            self.continuation_source_sha.as_deref(),
            self.journal_predecessor.is_some(),
        )?;
        validate_exact_source_agreement(
            file,
            self.recovery.as_ref().map(|r| r.source_sha.as_str()),
            self.new_crates.as_ref().map(|r| r.source_sha.as_str()),
            self.continuation_source_sha.as_deref(),
        )?;
        Ok(())
    }

    fn validate_recovery_selection(&self, file: &str) -> Result<(), ContractError> {
        if let Some(recovery) = &self.recovery {
            validate_recovery(file, recovery)?;
            if !self.publishable_workspace
                && (self.packages.len() != 1 || self.packages[0] != recovery.package)
            {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.release.recovery",
                    "recovery_requires_single_selected_package",
                ));
            }
        }
        Ok(())
    }

    fn validate_authentication(&self, file: &str) -> Result<(), ContractError> {
        let key = "stacks.rust.release.recovery";
        if self.registry == ReleaseRegistry::GitTag {
            return self.validate_git_tag(file);
        }
        if self.recovery.is_some() && self.new_crates.is_some() {
            return Err(ContractError::config(
                file,
                "new_crates",
                "conflicting_release_intents",
            ));
        }
        match (
            self.effective_authentication(),
            &self.recovery,
            &self.new_crates,
        ) {
            (ReleaseAuthentication::BootstrapToken, None, None) => {
                Err(ContractError::config(file, key, "missing_recovery_record"))
            }
            _ => Ok(()),
        }
    }
}

impl RustReleaseConfig {
    fn validate_git_tag(&self, file: &str) -> Result<(), ContractError> {
        if !self.publish_features.is_empty() {
            return Err(ContractError::config(
                file,
                "stacks.rust.release.publish_features",
                "git_tag_forbids_features",
            ));
        }
        if self.authentication.is_some() {
            return Err(ContractError::config(
                file,
                "stacks.rust.release.authentication",
                "contradictory_authentication",
            ));
        }
        if self.journal_predecessor.is_some() {
            return Err(ContractError::config(
                file,
                "stacks.rust.release.journal_predecessor",
                "git_tag_forbids_predecessor",
            ));
        }
        if self.recovery.is_some() {
            return Err(ContractError::config(
                file,
                "stacks.rust.release.recovery",
                "contradictory_authentication",
            ));
        }
        if self.publishable_workspace {
            return Err(ContractError::config(
                file,
                "stacks.rust.release.publishable_workspace",
                "git_tag_requires_packages",
            ));
        }
        Ok(())
    }
}

pub(super) fn validate_new_crates(
    file: &str,
    new_crates: Option<&NewCrateRelease>,
    packages: &[String],
    workspace: bool,
) -> Result<(), ContractError> {
    let Some(creation) = new_crates else {
        return Ok(());
    };
    if !super::release_validation::is_full_sha(&creation.source_sha)
        || creation.packages.is_empty()
        || creation.packages.len() > MAX_CRATE_RELEASE_PACKAGES
        || (!workspace && creation.packages.len() != packages.len())
        || creation.packages.iter().any(|(name, version)| {
            !super::release_validation::is_package_name(name)
                || (!workspace && !packages.contains(name))
                || version.len() > 150
                || !super::release_intent::is_cargo_version(version)
        })
    {
        return Err(ContractError::config(
            file,
            "new_crates",
            "invalid_new_crates",
        ));
    }
    Ok(())
}
