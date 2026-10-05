//! Stack-neutral workflow-tool identity (ver §2).
//!
//! The catalog MUST identify each workflow tool with exact version,
//! source, platforms, and digest. Adapters carry these records; the
//! checker compares pins against them.

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;
use crate::manifest_checks::check_sha256;

/// One workflow tool's pinned identity (ver §2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolIdentity {
    /// Registry tool name.
    pub name: String,
    /// Exact pinned version.
    pub version: String,
    /// Immutable source URL the pin was qualified from.
    pub source: String,
    /// Supported platform labels (sorted, non-empty).
    pub platforms: Vec<String>,
    /// SHA-256 of the qualified artifact (64 lowercase hex).
    pub digest: String,
}

impl ToolIdentity {
    /// Validate name, exact version, source, platforms, and digest.
    ///
    /// A validated identity is a trust record: the digest must be the
    /// SHA-256 of a qualified artifact, so the all-zero placeholder is
    /// rejected even though it is shape-valid hex (P03-8b).
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.name.trim().is_empty()
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(ContractError::config(file, "tool.name", "malformed_name"));
        }
        check_exact_version(&self.version, file)?;
        check_source(&self.source, file)?;
        if self.platforms.is_empty() {
            return Err(ContractError::config(
                file,
                "tool.platforms",
                "empty_platforms",
            ));
        }
        for platform in &self.platforms {
            if platform.trim().is_empty() || platform.contains(' ') {
                return Err(ContractError::config(
                    file,
                    "tool.platforms",
                    "malformed_platform",
                ));
            }
        }
        if self.platforms.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ContractError::config(
                file,
                "tool.platforms",
                "must_be_sorted_unique",
            ));
        }
        check_sha256(&self.digest, file, "tool.digest")?;
        if self.digest.bytes().all(|byte| byte == b'0') {
            return Err(ContractError::config(
                file,
                "tool.digest",
                "placeholder_digest",
            ));
        }
        Ok(())
    }
}

/// Check an exact numeric dotted version (`X.Y.Z...`, no suffixes).
fn check_exact_version(version: &str, file: &str) -> Result<(), ContractError> {
    let parts: Vec<&str> = version.split('.').collect();
    let valid = parts.len() >= 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if valid {
        Ok(())
    } else {
        Err(ContractError::config(
            file,
            "tool.version",
            "inexact_version",
        ))
    }
}

/// Check an immutable `https://` source URL (no floating segments).
fn check_source(source: &str, file: &str) -> Result<(), ContractError> {
    let Some(rest) = source.strip_prefix("https://") else {
        return Err(ContractError::config(
            file,
            "tool.source",
            "mutable_or_malformed_url",
        ));
    };
    let valid = !rest.is_empty()
        && !source.contains(' ')
        && !rest.split('/').any(|seg| seg.is_empty() || seg == "latest");
    if valid {
        Ok(())
    } else {
        Err(ContractError::config(
            file,
            "tool.source",
            "mutable_or_malformed_url",
        ))
    }
}
