//! Opt-in single-package Rust binary release configuration.
//!
//! The first supported release target is native Apple Silicon macOS. The
//! package and binary are explicit; the workflow never expands a workspace.

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

/// Only target supported by the initial consumer binary release path.
pub const CONSUMER_BINARY_TARGET: &str = "aarch64-apple-darwin";

/// Rust binary release policy (`[stacks.rust.binary_release]`).
///
/// Disabled by default. This is separate from crates.io release-plz and
/// intentionally carries no release command, tag, or action overrides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustBinaryReleaseConfig {
    /// Whether to emit the generated consumer binary release workflow.
    #[serde(default)]
    pub enabled: bool,
    /// Repo-relative workspace manifest, defaulting to `Cargo.toml`.
    #[serde(default = "default_manifest_path")]
    pub manifest_path: String,
    /// Exact Cargo package to build; required when enabled.
    #[serde(default)]
    pub package: String,
    /// Exact Cargo binary target to build; required when enabled.
    #[serde(default)]
    pub bin: String,
}

fn default_manifest_path() -> String {
    "Cargo.toml".to_owned()
}

impl Default for RustBinaryReleaseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            manifest_path: default_manifest_path(),
            package: String::new(),
            bin: String::new(),
        }
    }
}

impl RustBinaryReleaseConfig {
    /// Validate the fixed target and explicit one-package/one-bin selection.
    ///
    /// # Errors
    ///
    /// Returns a config error for unsafe paths or names, or a missing
    /// package/bin selection when enabled.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        validate_manifest_path(file, &self.manifest_path)?;
        validate_name(file, "package", &self.package, self.enabled)?;
        validate_name(file, "bin", &self.bin, self.enabled)?;
        Ok(())
    }
}

fn validate_name(file: &str, field: &str, name: &str, required: bool) -> Result<(), ContractError> {
    let key = format!("stacks.rust.binary_release.{field}");
    if name.is_empty() {
        return if required {
            Err(ContractError::config(file, key, "required_when_enabled"))
        } else {
            Ok(())
        };
    }
    let valid = name.len() <= 100
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if !valid {
        return Err(ContractError::config(file, key, "bad_cargo_target_name"));
    }
    Ok(())
}

fn validate_manifest_path(file: &str, path: &str) -> Result<(), ContractError> {
    let key = "stacks.rust.binary_release.manifest_path";
    let valid = path == "Cargo.toml"
        || (path.ends_with("/Cargo.toml")
            && !path.starts_with('/')
            && path
                .split('/')
                .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
            && path.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/')
            }));
    if !valid {
        return Err(ContractError::config(file, key, "bad_manifest_path"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CONSUMER_BINARY_TARGET, RustBinaryReleaseConfig};

    #[test]
    fn defaults_disabled_and_requires_one_explicit_package_and_binary() {
        let config = RustBinaryReleaseConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.manifest_path, "Cargo.toml");
        assert!(config.validate("config.toml").is_ok());

        let enabled = RustBinaryReleaseConfig {
            enabled: true,
            manifest_path: "Cargo.toml".to_owned(),
            package: "repo-scan".to_owned(),
            bin: "repo-scan".to_owned(),
        };
        assert!(enabled.validate("config.toml").is_ok());
        assert_eq!(CONSUMER_BINARY_TARGET, "aarch64-apple-darwin");
    }

    #[test]
    fn rejects_missing_or_unsafe_selection() {
        let missing = RustBinaryReleaseConfig {
            enabled: true,
            ..RustBinaryReleaseConfig::default()
        };
        let err = missing
            .validate("config.toml")
            .expect_err("selection required");
        assert!(err.to_string().contains("required_when_enabled"));

        let unsafe_name = RustBinaryReleaseConfig {
            enabled: true,
            manifest_path: "Cargo.toml".to_owned(),
            package: "repo-scan;id".to_owned(),
            bin: "repo-scan".to_owned(),
        };
        let err = unsafe_name
            .validate("config.toml")
            .expect_err("shell metacharacters rejected");
        assert!(err.to_string().contains("bad_cargo_target_name"));
    }

    #[test]
    fn rejects_manifest_paths_that_escape_or_change_file_kind() {
        for path in [
            "../Cargo.toml",
            "a//Cargo.toml",
            "Cargo.lock",
            "/tmp/Cargo.toml",
        ] {
            let config = RustBinaryReleaseConfig {
                manifest_path: path.to_owned(),
                ..RustBinaryReleaseConfig::default()
            };
            assert!(config.validate("config.toml").is_err(), "{path}");
        }
    }
}
