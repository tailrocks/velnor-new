//! Shared Rust policy identity (`[stacks.rust.policy]`).
//!
//! The adopter pins the exact `tailrocks/rust-repository-policy`
//! release the policy lane materializes: package version plus the
//! release-tarball SHA-256, verified at runtime before Alint runs.
//! The profile is mandatory and single-valued: only `rust-strict-v1`
//! exists, and the lane verifies the materialized package carries it.
//! Generic Rust capability: any policy (consumer or Velnor) may carry
//! this table; presence opts the repository into the policy lane.
use serde::{Deserialize, Serialize};
use velnor_actions_contract::errors::ContractError;

/// Shared Rust policy identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustPolicyConfig {
    /// Pinned policy release version (`major.minor.patch`, numeric only).
    pub version: String,
    /// SHA-256 of the `rust-repository-policy-<version>.tar.gz` asset.
    pub sha256: String,
    /// Mandatory strict profile (the only profile that exists).
    pub profile: RustPolicyProfile,
}

/// Strict profile selector (single-valued by construction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RustPolicyProfile {
    /// The strict repository profile.
    RustStrictV1,
}

impl RustPolicyProfile {
    /// Profile file name inside the materialized package.
    #[must_use]
    pub fn file_name(&self) -> &'static str {
        match self {
            Self::RustStrictV1 => "rust-strict-v1.yml",
        }
    }
}

impl RustPolicyConfig {
    /// Validate version exactness, digest shape, and profile presence.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if !is_exact_version(&self.version) {
            return Err(ContractError::config(
                file,
                "stacks.rust.policy.version",
                format!("bad_version:{}", self.version),
            ));
        }
        if !is_sha256(&self.sha256) {
            return Err(ContractError::config(
                file,
                "stacks.rust.policy.sha256",
                "bad_sha256",
            ));
        }
        Ok(())
    }
}

/// Strict release version: exactly three dot-separated numeric parts.
fn is_exact_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Tarball digest: 64 lowercase hex characters (never echoed: the value
/// is a pin, not a diagnostic).
fn is_sha256(sha: &str) -> bool {
    sha.len() == 64
        && sha
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests;
