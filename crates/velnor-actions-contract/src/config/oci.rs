//! Explicit native OCI delivery inputs; no inferred publication scope.

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Supported native Linux build platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OciPlatform {
    /// Native x86-64 runner.
    #[serde(rename = "linux/amd64")]
    Amd64,
    /// Native ARM64 runner.
    #[serde(rename = "linux/arm64")]
    Arm64,
}

impl OciPlatform {
    /// OCI architecture name.
    #[must_use]
    pub fn arch(self) -> &'static str {
        match self {
            Self::Amd64 => "amd64",
            Self::Arm64 => "arm64",
        }
    }
    /// Versioned native GitHub runner label.
    #[must_use]
    pub fn runner(self) -> &'static str {
        match self {
            Self::Amd64 => "ubuntu-24.04",
            Self::Arm64 => "ubuntu-24.04-arm",
        }
    }
}

/// One explicitly publishable image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OciImage {
    /// Stable image and artifact identity.
    pub id: String,
    /// Registry repository, without a mutable tag or digest.
    pub image: String,
    /// Repository-relative build context.
    pub context: String,
    /// Repository-relative Dockerfile.
    pub dockerfile: String,
    /// Exact runnable platform set.
    pub platforms: Vec<OciPlatform>,
    /// Fetch Git LFS content during checkout.
    #[serde(default)]
    pub lfs: bool,
    /// Earlier image identities whose version indexes must exist first.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Literal build arguments; VERSION is bound by the generator.
    #[serde(default)]
    pub build_args: BTreeMap<String, String>,
}

/// Native Docker publication policy, disabled unless explicitly enabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OciReleaseConfig {
    /// Emit publication workflow.
    #[serde(default)]
    pub enabled: bool,
    /// Docker registry host, for example docker.io.
    pub registry: String,
    /// Closed credential source for the selected registry.
    pub authentication: RegistryAuthentication,
    /// Exact image allowlist in dependency order.
    #[serde(default)]
    pub images: Vec<OciImage>,
}

/// Registry credentials never contain arbitrary GitHub expressions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RegistryAuthentication {
    /// Explicitly named registry username/password secrets.
    NamedSecrets {
        /// GitHub secret name containing registry username.
        username_secret: String,
        /// GitHub secret name containing registry password/token.
        password_secret: String,
    },
    /// GitHub actor and scoped job token for ghcr.io only.
    GithubToken,
}

fn component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn secret(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.to_ascii_uppercase().starts_with("GITHUB_")
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

impl OciReleaseConfig {
    /// Validate all publication identities and reject cycles by ordered edges.
    /// # Errors
    /// Returns config diagnostics for unsafe or contradictory publication inputs.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let invalid = |problem| ContractError::config(file, "delivery.oci", problem);
        if self.enabled && self.images.is_empty() {
            return Err(invalid("empty_images"));
        }
        if self.images.len() > 64 {
            return Err(invalid("too_many_images"));
        }
        if self.registry.is_empty()
            || !self
                .registry
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'))
            || self
                .registry
                .split('.')
                .any(|label| label.is_empty() || label.starts_with('-') || label.ends_with('-'))
        {
            return Err(invalid("bad_registry"));
        }
        match &self.authentication {
            RegistryAuthentication::NamedSecrets {
                username_secret,
                password_secret,
            } => {
                if !secret(username_secret) || !secret(password_secret) {
                    return Err(invalid("bad_registry_secret"));
                }
            }
            RegistryAuthentication::GithubToken if self.registry != "ghcr.io" => {
                return Err(invalid("github_token_registry_mismatch"));
            }
            RegistryAuthentication::GithubToken => {}
        }
        let mut seen = BTreeSet::new();
        let mut names = BTreeSet::new();
        for image in &self.images {
            image.validate(file)?;
            if self.registry == "docker.io"
                && image.image.split('/').next().is_some_and(|host| {
                    (host.contains('.') || host == "localhost") && host != "docker.io"
                })
            {
                return Err(invalid("image_registry_mismatch"));
            }
            if self.registry != "docker.io"
                && !image.image.starts_with(&format!("{}/", self.registry))
            {
                return Err(invalid("image_registry_mismatch"));
            }
            if !names.insert(&image.image) {
                return Err(invalid("duplicate_image"));
            }
            for dependency in &image.depends_on {
                if !seen.contains(dependency) {
                    return Err(invalid("unordered_or_unknown_dependency"));
                }
            }
            if !seen.insert(image.id.clone()) {
                return Err(invalid("duplicate_image_id"));
            }
        }
        Ok(())
    }
}

impl OciImage {
    /// Validate render-safe identity, paths, platform scope, and literal arguments.
    /// # Errors
    /// Returns a diagnostic naming the offending image.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let key = format!("delivery.oci.images.{}", self.id);
        let invalid = |problem| ContractError::config(file, &key, problem);
        if !component(&self.id) {
            return Err(invalid("bad_id"));
        }
        if self.image.len() > 256
            || self.image.split('/').count() < 2
            || !self.image.split('/').all(|s| {
                !s.is_empty()
                    && s != "."
                    && s != ".."
                    && !s.starts_with('-')
                    && s.bytes().all(|b| {
                        b.is_ascii_lowercase()
                            || b.is_ascii_digit()
                            || matches!(b, b'-' | b'_' | b'.')
                    })
            })
        {
            return Err(invalid("bad_image_repository"));
        }
        if (self.context != "." && !super::is_valid_workload_path(&self.context))
            || !super::is_valid_workload_path(&self.dockerfile)
        {
            return Err(invalid("bad_build_path"));
        }
        let platforms: BTreeSet<_> = self.platforms.iter().collect();
        if platforms.is_empty() || platforms.len() != self.platforms.len() {
            return Err(invalid("bad_platform_set"));
        }
        let dependencies: BTreeSet<_> = self.depends_on.iter().collect();
        if dependencies.len() != self.depends_on.len() {
            return Err(invalid("duplicate_dependency"));
        }
        for (name, value) in &self.build_args {
            if name == "VERSION"
                || name.is_empty()
                || !name
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || value.len() > 4096
                || value.chars().any(char::is_control)
                || value.contains("${{")
            {
                return Err(invalid("unsafe_build_argument"));
            }
        }
        Ok(())
    }
}
