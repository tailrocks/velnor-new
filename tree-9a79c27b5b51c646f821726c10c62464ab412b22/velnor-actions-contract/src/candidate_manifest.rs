//! Per-target candidate artifact manifest (bootstrap contract §4 step 4).

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;
use crate::manifest::{check_schema, check_sha256, check_target, is_lower_hex};

/// Per-target candidate artifact manifest (bootstrap contract §4 step 4).
///
/// Written once by the candidate build; qualification downloads the exact
/// bytes described here and never rebuilds them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateArtifactManifest {
    /// Manifest schema version; must be 1.
    pub schema: u32,
    /// Source commit SHA that produced the candidate.
    pub commit: String,
    /// Target triple the candidate was built for.
    pub target: String,
    /// Toolchain identity (`rust@<exact>+mr-boxington@<exact>`).
    pub toolchain: String,
    /// SHA-256 of the candidate binary (64 lowercase hex).
    pub sha256: String,
}

impl CandidateArtifactManifest {
    /// Schema version this contract accepts.
    pub const SCHEMA: u32 = 1;

    /// Render the canonical JSON bytes uploaded beside the candidate binary.
    #[must_use]
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Validate schema, commit, target, toolchain, and digest.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        check_schema(self.schema)?;
        if self.commit.len() != 40 || !is_lower_hex(&self.commit) {
            return Err(ContractError::config(file, "commit", "malformed_commit"));
        }
        check_target(&self.target, file, "target")?;
        if !crate::targets::is_supported_target(&self.target) {
            return Err(ContractError::config(
                file,
                "target",
                format!("unsupported_target:{}", self.target),
            ));
        }
        if self.toolchain.trim().is_empty() {
            return Err(ContractError::config(file, "toolchain", "empty_toolchain"));
        }
        check_sha256(&self.sha256, file, "sha256")?;
        Ok(())
    }
}
