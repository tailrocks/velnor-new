//! Per-target candidate artifact manifest (bootstrap contract §4 step 4).

use serde::{Deserialize, Serialize};

use crate::manifest_checks::{check_commit, check_schema, check_sha256, check_target};
use velnor_actions_contract::errors::ContractError;

/// Per-target candidate artifact manifest (bootstrap contract §4 step 4).
///
/// Written once by the candidate build; qualification downloads the exact
/// bytes described here and never rebuilds them.
///
/// Enforcement split (S10/D6 residual): consumers verify this manifest
/// with the emitted shell script before running any candidate command
/// (cache-contract §4); the merge re-checks ONLY the commit binding
/// through the head-bound attestation (`commit == plan.head`) plus the
/// candidate job's needs conclusion. `sha256`/`toolchain`/`target` are
/// consumer-asserted audit data at merge time — a consumer that skips
/// verification is outside the merge's view, by design (the merge never
/// executes the candidate and cannot recompute its digest).
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
        check_commit(&self.commit, file, "commit")?;
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

/// Require an exact release version (`X.Y.Z`, gaps §E).
///
/// Pre-release suffixes, missing components, and empty versions fail
/// with `non_release_build`: only exact releases may publish.
/// # Errors
pub fn require_release_version(version: &str, file: &str) -> Result<(), ContractError> {
    if is_release_version(version) {
        Ok(())
    } else {
        Err(ContractError::config(file, "version", "non_release_build"))
    }
}

/// True only for exact numeric `X.Y.Z` release versions.
fn is_release_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}
