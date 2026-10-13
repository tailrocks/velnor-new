//! Final aggregate and candidate qualification reports.
use crate::errors::ContractError;
use crate::ids::{
    artifact_id_for_candidate, artifact_id_for_final, target_key, validate_artifact_id,
    validate_report_id, validate_run_key,
};
use serde::{Deserialize, Serialize};

/// Final aggregate report (`final-<run-key>`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalReport {
    /// Report schema version; must be 1.
    pub schema: u32,
    /// Derived final report ID.
    pub report_id: String,
    /// Run key.
    pub run_key: String,
    /// Validated plan ID.
    pub plan_id: String,
    /// Every expected matrix report ID (sorted).
    pub expected_report_ids: Vec<String>,
    /// Every downloaded artifact name (sorted).
    pub downloaded_artifact_ids: Vec<String>,
    /// Required non-matrix job results.
    pub required_job_results: Vec<RequiredJobResult>,
    /// Computed final result.
    pub status: FinalStatus,
    /// Status counts.
    pub counts: FinalCounts,
}
/// One required job conclusion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredJobResult {
    /// Job ID.
    pub job_id: String,
    /// Job conclusion.
    pub conclusion: String,
}
/// Final gate result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalStatus {
    /// Valid plan with zero obligations.
    NoWork,
    /// Every obligation satisfied.
    Passed,
    /// A required task failed.
    Failed,
    /// A required task was cancelled.
    Cancelled,
    /// A selected entry lacks a valid report.
    NotRun,
    /// Planning or validation failed.
    PlanningFailed,
}
/// Final status counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalCounts {
    /// Selected count.
    pub selected: u32,
    /// Reused count.
    pub reused: u32,
    /// Executed count.
    pub executed: u32,
    /// Empty-partition count.
    pub empty_partition: u32,
    /// Baseline-covered count.
    pub covered: u32,
    /// Failed count.
    pub failed: u32,
    /// Cancelled count.
    pub cancelled: u32,
    /// Blocked count.
    pub blocked: u32,
    /// Not-run count.
    pub not_run: u32,
}
/// Candidate validation report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateReport {
    /// Report schema version; must be 1.
    pub schema: u32,
    /// Derived candidate report ID.
    pub report_id: String,
    /// Run key.
    pub run_key: String,
    /// Candidate source commit (40 lowercase hex).
    pub source_commit: String,
    /// Candidate target triple.
    pub target: String,
    /// Candidate artifact SHA-256 (64 lowercase hex).
    pub artifact_sha256: String,
    /// Candidate generator version.
    pub generator_version: String,
    /// Qualification status.
    pub status: CandidateStatus,
    /// Executed checks.
    pub checks: Vec<String>,
}
/// Candidate qualification status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    /// Qualification passed.
    Passed,
    /// Qualification failed.
    Failed,
    /// Qualification cancelled.
    Cancelled,
}
/// Derive the final report ID `final-<run-key>`.
/// # Errors
pub fn final_report_id_for_run(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    Ok(format!("final-{run_key}"))
}
/// Validate a final report ID.
/// # Errors
pub fn validate_final_report_id(value: &str) -> Result<(), ContractError> {
    let Some(run_key) = value.strip_prefix("final-") else {
        return Err(ContractError::identity(
            "report_id",
            "malformed_final_report_id",
        ));
    };
    validate_run_key(run_key)
        .map_err(|_| ContractError::identity("report_id", "malformed_final_report_id"))
}
/// Derive the candidate report ID `candidate-<run-key>-<target-key>`.
/// # Errors
pub fn candidate_report_id_for_run(run_key: &str, target: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    Ok(format!("candidate-{run_key}-{}", target_key(target)?))
}
/// Validate a candidate report ID.
/// # Errors
pub fn validate_candidate_report_id(value: &str) -> Result<(), ContractError> {
    let Some(rest) = value.strip_prefix("candidate-") else {
        return Err(ContractError::identity(
            "report_id",
            "malformed_candidate_report_id",
        ));
    };
    for (index, _) in rest.match_indices('-') {
        let head = &rest[..index];
        let tail = &rest[index + 1..];
        if validate_run_key(head).is_ok() && target_key(tail).is_ok_and(|key| key == tail) {
            return Ok(());
        }
    }
    Err(ContractError::identity(
        "report_id",
        "malformed_candidate_report_id",
    ))
}
impl FinalReport {
    /// Report schema version.
    pub const SCHEMA: u32 = 1;
    /// Validate schema, derived IDs, sorting, and artifact names.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema)?;
        validate_final_report_id(&self.report_id)?;
        validate_run_key(&self.run_key)?;
        crate::ids::validate_plan_id(&self.plan_id)?;
        if !is_sorted(&self.expected_report_ids) || !is_sorted(&self.downloaded_artifact_ids) {
            return Err(ContractError::identity("final_report", "must_be_sorted"));
        }
        for report_id in &self.expected_report_ids {
            validate_report_id(report_id)?;
        }
        for artifact_id in &self.downloaded_artifact_ids {
            validate_artifact_id(artifact_id)?;
        }
        Ok(())
    }
    /// Derive the expected final artifact name for this report.
    /// # Errors
    pub fn artifact_id(&self) -> Result<String, ContractError> {
        artifact_id_for_final(&self.run_key)
    }
}
impl CandidateReport {
    /// Report schema version.
    pub const SCHEMA: u32 = 1;
    /// Validate schema, derived ID, commit, target, and digest shapes.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema)?;
        validate_candidate_report_id(&self.report_id)?;
        validate_run_key(&self.run_key)?;
        if self.source_commit.len() != 40 || !is_lower_hex(&self.source_commit) {
            return Err(ContractError::identity("source_commit", "malformed_commit"));
        }
        target_key(&self.target)?;
        if self.artifact_sha256.len() != 64 || !is_lower_hex(&self.artifact_sha256) {
            return Err(ContractError::identity(
                "artifact_sha256",
                "malformed_sha256",
            ));
        }
        if self.generator_version.trim().is_empty() {
            return Err(ContractError::identity(
                "generator_version",
                "empty_version",
            ));
        }
        Ok(())
    }
    /// Derive the expected candidate artifact name for this report.
    /// # Errors
    pub fn artifact_id(&self) -> Result<String, ContractError> {
        artifact_id_for_candidate(&self.run_key, &self.target)
    }
}
/// Check a schema-1 version marker.
fn check_schema(schema: u32) -> Result<(), ContractError> {
    if schema != 1 {
        return Err(ContractError::UnsupportedSchema {
            field: "schema",
            found: schema.to_string(),
            expected: "1",
        });
    }
    Ok(())
}
/// Check a string list is sorted.
fn is_sorted(list: &[String]) -> bool {
    list.windows(2).all(|pair| pair[0] <= pair[1])
}
/// Check lowercase hex.
fn is_lower_hex(text: &str) -> bool {
    text.bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
