//! Typed Rust release configuration (`[stacks.rust.release]`).
//!
//! Pure data plus validation: no subprocess, registry, or YAML access.
//! Unknown fields are rejected by serde; [`RustReleaseConfig::validate`]
//! reports file, key path, and problem for every other violation.

use super::release_validation::{
    validate_bootstrap, validate_environment, validate_manifest_path, validate_packages,
    validate_tag_name, validate_version_groups,
};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Release authentication mode (exactly one; modes never mix).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseAuthentication {
    /// crates.io Trusted Publishing via OIDC (`id-token: write` job).
    #[default]
    TrustedPublishing,
    /// Short-lived bootstrap API token for first publication only.
    BootstrapToken,
}

/// Where a release lands: a registry, or only git tags and GitHub releases.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseRegistry {
    /// Publish to crates.io through release-plz.
    #[default]
    CratesIo,
    /// Create tags and GitHub releases only (`publish = false`, git-only).
    ///
    /// Consumers pin `git` dependencies to the tag. A git dependency checks
    /// out the whole repository, which keeps paths such as
    /// `embed_migrations!("../../backend/eth-flyway/...")` valid.
    GitTag,
}

/// Exact first-publication authorization (bootstrap mode only).
///
/// Immutable record: package, version, and source SHA. A dispatch must never
/// widen this record; retire it after the OIDC handover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapRelease {
    /// Exact package authorized for first publication.
    pub package: String,
    /// Exact version authorized (`major.minor.patch`, numeric only).
    pub version: String,
    /// Full lowercase-hex immutable source SHA.
    pub source_sha: String,
}

/// Rust release policy (`[stacks.rust.release]`).
///
/// Disabled by default. There are no shell/YAML/`uses` override fields by
/// design; unknown keys fail deserialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustReleaseConfig {
    /// Publication enabled; `false` emits no release work.
    #[serde(default)]
    pub enabled: bool,
    /// Repo-relative workspace manifest (default `Cargo.toml`).
    #[serde(default = "default_manifest_path")]
    pub manifest_path: String,
    /// Explicit package allowlist; sorted, unique, never auto-expanded.
    #[serde(default)]
    pub packages: Vec<String>,
    /// Explicit publishable-workspace opt-in; exclusive with `packages`.
    #[serde(default)]
    pub publishable_workspace: bool,
    /// Protected GitHub environment bound to publishing jobs.
    #[serde(default = "default_environment")]
    pub environment: String,
    /// Release destination (default crates.io).
    #[serde(default, skip_serializing_if = "is_default_registry")]
    pub registry: ReleaseRegistry,
    /// Authentication mode; absent means trusted publishing on crates.io.
    /// Forbidden in `git-tag` mode, which holds no registry credential.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authentication: Option<ReleaseAuthentication>,
    /// Run release-plz release-pr preparation (default true; crates.io only,
    /// `git-tag` releases are prepared by a maintainer's version bump).
    #[serde(default = "default_true")]
    pub release_pr: bool,
    /// Package-qualified tag template carrying package and version vars.
    #[serde(default = "default_tag_name")]
    pub tag_name: String,
    /// Bootstrap authorization; required iff bootstrap-token mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bootstrap: Option<BootstrapRelease>,
    /// Named version groups with non-lockstep semantics: members coordinate
    /// versioning through release-plz, but unchanged members are not forced
    /// to release. Members must be listed in `packages`; under the
    /// publishable-workspace opt-in, emission enforces set membership.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub version_groups: BTreeMap<String, Vec<String>>,
}

/// Serde skip predicate for the default registry.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip predicates take a reference"
)]
fn is_default_registry(registry: &ReleaseRegistry) -> bool {
    *registry == ReleaseRegistry::default()
}

/// Default workspace manifest path.
fn default_manifest_path() -> String {
    "Cargo.toml".to_owned()
}

/// Default protected publishing environment name: `release`, matching the
/// crates.io trusted-publishing docs and majority ecosystem practice.
fn default_environment() -> String {
    "release".to_owned()
}

/// Default `release_pr` value.
fn default_true() -> bool {
    true
}

/// Default package-qualified tag template.
fn default_tag_name() -> String {
    "{{ package }}-v{{ version }}".to_owned()
}

impl Default for RustReleaseConfig {
    /// Disabled release with safe defaults (no packages selected).
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
    /// Effective authentication: the declared mode, else trusted publishing.
    #[must_use]
    pub fn effective_authentication(&self) -> ReleaseAuthentication {
        self.authentication.unwrap_or_default()
    }

    /// Validate shape, safety, and mode coherence.
    ///
    /// Runs whether enabled or not so drafted config stays safe; `enabled`
    /// additionally requires one scope: an allowlist or the publishable
    /// opt-in. Never expands either.
    /// # Errors
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

    /// Reject contradictory authentication/authorization combinations.
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
    /// `git-tag` mode: no registry, no credential, explicit packages only.
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
