use super::release_validation::{
    validate_bootstrap, validate_environment, validate_manifest_path, validate_packages,
    validate_tag_name, validate_version_groups,
};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
pub struct BootstrapRelease {
    pub package: String,

    pub version: String,

    pub source_sha: String,
}

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

    #[serde(default = "default_environment")]
    pub environment: String,

    #[serde(default, skip_serializing_if = "is_default_registry")]
    pub registry: ReleaseRegistry,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authentication: Option<ReleaseAuthentication>,

    #[serde(default = "default_true")]
    pub release_pr: bool,

    #[serde(default = "default_tag_name")]
    pub tag_name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bootstrap: Option<BootstrapRelease>,

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
            environment: default_environment(),
            registry: ReleaseRegistry::default(),
            authentication: None,
            release_pr: true,
            tag_name: default_tag_name(),
            bootstrap: None,
            version_groups: BTreeMap::new(),
        }
    }
}

impl RustReleaseConfig {
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
        validate_environment(file, &self.environment)?;
        self.validate_authentication(file)?;
        validate_tag_name(file, &self.tag_name)?;
        if let Some(bootstrap) = &self.bootstrap {
            validate_bootstrap(file, bootstrap)?;
        }
        validate_version_groups(
            file,
            &self.version_groups,
            &self.packages,
            self.publishable_workspace,
        )?;
        Ok(())
    }

    fn validate_authentication(&self, file: &str) -> Result<(), ContractError> {
        let key = "stacks.rust.release.bootstrap";
        if self.registry == ReleaseRegistry::GitTag {
            return self.validate_git_tag(file);
        }
        match (self.effective_authentication(), &self.bootstrap) {
            (ReleaseAuthentication::BootstrapToken, None) => {
                Err(ContractError::config(file, key, "missing_bootstrap_record"))
            }
            (ReleaseAuthentication::TrustedPublishing, Some(_)) => Err(ContractError::config(
                file,
                key,
                "contradictory_authentication",
            )),
            _ => Ok(()),
        }
    }
}

impl RustReleaseConfig {
    fn validate_git_tag(&self, file: &str) -> Result<(), ContractError> {
        if self.authentication.is_some() {
            return Err(ContractError::config(
                file,
                "stacks.rust.release.authentication",
                "contradictory_authentication",
            ));
        }
        if self.bootstrap.is_some() {
            return Err(ContractError::config(
                file,
                "stacks.rust.release.bootstrap",
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
