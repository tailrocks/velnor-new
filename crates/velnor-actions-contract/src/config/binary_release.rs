//! Typed Rust binary GitHub Release configuration.
//!
//! This is separate from [`super::release::RustReleaseConfig`], which
//! publishes crate archives to a Cargo registry.

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Rust binary release policy (`[stacks.rust.binary_release]`).
///
/// Disabled by default. A release names exactly one Cargo package and
/// binary target; no command, shell, YAML, action, or target overrides exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryReleaseConfig {
    /// Whether to emit the trusted scheduled GitHub Release workflow.
    #[serde(default)]
    pub enabled: bool,
    /// Repo-relative workspace manifest (default `Cargo.toml`).
    #[serde(default = "default_manifest_path")]
    pub manifest_path: String,
    /// Exact Cargo package whose binary is released.
    #[serde(default)]
    pub package: String,
    /// Exact Cargo binary target; defaults to the package name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
    /// Optional compile-time environment variable receiving the source SHA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_commit_env: Option<String>,
}

impl Default for RustBinaryReleaseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            manifest_path: default_manifest_path(),
            package: String::new(),
            binary: None,
            source_commit_env: None,
        }
    }
}

impl RustBinaryReleaseConfig {
    /// Validate shape and shell/path safety.
    ///
    /// Runs even while disabled so drafted config stays safe. Enabling the
    /// section requires an explicit Cargo package.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        validate_manifest_path(file, &self.manifest_path)?;
        if self.package.is_empty() {
            if self.enabled {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.binary_release.package",
                    "missing_package",
                ));
            }
        } else if !super::release::is_package_name(&self.package) {
            return Err(ContractError::config(
                file,
                "stacks.rust.binary_release.package",
                format!("unsafe_package:{}", self.package),
            ));
        }
        if let Some(binary) = &self.binary
            && !super::release::is_package_name(binary)
        {
            return Err(ContractError::config(
                file,
                "stacks.rust.binary_release.binary",
                format!("unsafe_binary:{binary}"),
            ));
        }
        if let Some(name) = &self.source_commit_env
            && !is_env_name(name)
        {
            return Err(ContractError::config(
                file,
                "stacks.rust.binary_release.source_commit_env",
                "bad_environment_name",
            ));
        }
        Ok(())
    }

    /// Binary target, defaulting to the selected package name.
    #[must_use]
    pub fn binary_name(&self) -> &str {
        self.binary.as_deref().unwrap_or(&self.package)
    }
}

fn is_env_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    let valid = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    valid
        && !matches!(
            value,
            "PATH"
                | "HOME"
                | "SOURCE_SHA"
                | "RELEASE_VERSION"
                | "RELEASE_TAG"
                | "RUSTUP_TOOLCHAIN"
                | "GH_TOKEN"
        )
        && !["GITHUB_", "RUNNER_", "ACTIONS_", "MISE_", "CARGO_"]
            .iter()
            .any(|prefix| value.starts_with(prefix))
}

fn default_manifest_path() -> String {
    "Cargo.toml".to_owned()
}

fn validate_manifest_path(file: &str, path: &str) -> Result<(), ContractError> {
    let key = "stacks.rust.binary_release.manifest_path";
    if path.is_empty()
        || !path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_'))
    {
        return Err(ContractError::config(file, key, "bad_manifest_path"));
    }
    if path.starts_with('/')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "..")
    {
        return Err(ContractError::config(file, key, "non_relative_manifest"));
    }
    if path != "Cargo.toml" && !path.ends_with("/Cargo.toml") {
        return Err(ContractError::config(file, key, "missing_cargo_toml"));
    }
    Ok(())
}
