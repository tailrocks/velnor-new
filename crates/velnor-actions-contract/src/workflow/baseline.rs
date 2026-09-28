//! Trusted-baseline evidence records.
use crate::canonical::validate_digest;
use crate::errors::ContractError;
use crate::ids::validate_artifact_id;
use serde::{Deserialize, Serialize};
/// Baseline evidence record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanBaseline {
    /// Baseline status.
    pub status: BaselineStatus,
    /// Trusted base commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    /// Baseline numeric run ID (proof only, never identity input).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<u64>,
    /// Baseline numeric artifact ID (GitHub API identity).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<u64>,
    /// Exact baseline artifact name (derived-name shape).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_name: Option<String>,
    /// Baseline manifest digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_digest: Option<String>,
    /// Unavailable reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
/// Baseline status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineStatus {
    /// Baseline evidence used.
    Used,
    /// No usable baseline.
    Unavailable,
}
/// Baseline proof for a covered obligation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineProof {
    /// Source commit of the proof.
    pub source_commit: String,
    /// Numeric GitHub run ID (proof only, never identity input).
    pub run_id: u64,
    /// Numeric GitHub artifact ID.
    pub artifact_id: u64,
    /// Derived baseline artifact name.
    pub artifact_name: String,
    /// Proof manifest digest.
    pub manifest_digest: String,
}
impl BaselineProof {
    /// Validate the artifact name shape and manifest digest.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.artifact_name.trim().is_empty() {
            return Err(ContractError::identity(
                "baseline_proof.artifact_name",
                "empty_name",
            ));
        }
        validate_artifact_id(&self.artifact_name)?;
        if self.manifest_digest.trim().is_empty() {
            return Err(ContractError::identity(
                "baseline_proof.manifest_digest",
                "empty_digest",
            ));
        }
        validate_digest(&self.manifest_digest)?;
        Ok(())
    }
}
