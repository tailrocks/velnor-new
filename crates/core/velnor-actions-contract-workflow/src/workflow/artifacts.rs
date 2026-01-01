//! Plan-artifact bytes and matrix agreement (wf §4).
//!
//! The plan artifact MUST contain `plan.json` plus `matrix.json`
//! (`{"include":[...]}`); final MUST validate canonical byte agreement.
//! These pure helpers render the bytes and compare them; planners write
//! the files and final downloads them.

use crate::workflow::plan::{Plan, PlanMatrix};
use velnor_actions_contract::canonical::canonical_json_bytes;
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract::strict_json::{MAX_UNTRUSTED_DOCUMENT_BYTES, parse_strict_json_bytes};

/// Plan document filename inside the plan artifact.
pub const PLAN_JSON_FILENAME: &str = "plan.json";
/// Matrix document filename inside the plan artifact.
pub const MATRIX_JSON_FILENAME: &str = "matrix.json";
/// Final verdict filename written by merge, uploaded by the final job.
pub const FINAL_JSON_FILENAME: &str = "final-report.json";
/// Head-bound candidate attestation filename (S3).
///
/// Written by the candidate job beside the candidate manifest (same
/// artifact), downloaded by the final job under
/// [`CANDIDATE_EVIDENCE_SUBDIR`], and re-checked at merge: its
/// `commit` must equal the plan head.
pub const CANDIDATE_ATTESTATION_FILENAME: &str = "candidate-attestation.json";
/// Run-dir subdirectory receiving the downloaded candidate artifact.
pub const CANDIDATE_EVIDENCE_SUBDIR: &str = "candidate";

/// Render the canonical `plan.json` bytes for a validated plan.
/// # Errors
pub fn plan_json_bytes(plan: &Plan) -> Result<Vec<u8>, ContractError> {
    plan.validate()?;
    canonical_json_bytes(plan)
}

/// Render the canonical `matrix.json` (`{"include":[...]}`) bytes.
/// # Errors
pub fn matrix_json_bytes(matrix: &PlanMatrix) -> Result<Vec<u8>, ContractError> {
    canonical_json_bytes(matrix)
}

/// Check downloaded `matrix.json` bytes agree with the plan matrix.
///
/// Both sides are compared as canonical bytes, so pretty-printed
/// downloads still agree; any content difference fails closed. The
/// download parses through the bounded strict boundary: oversize,
/// non-UTF-8, and duplicate-key documents are rejected.
/// # Errors
pub fn check_matrix_agreement(
    plan_matrix: &PlanMatrix,
    matrix_file_bytes: &[u8],
) -> Result<(), ContractError> {
    let expect = canonical_json_bytes(plan_matrix)?;
    let value = parse_strict_json_bytes(matrix_file_bytes, MAX_UNTRUSTED_DOCUMENT_BYTES)
        .map_err(|err| ContractError::identity("matrix.json", err.to_string()))?;
    let actual = canonical_json_bytes(&value)?;
    if expect == actual {
        Ok(())
    } else {
        Err(ContractError::identity(
            "matrix.json",
            "matrix_agreement_mismatch",
        ))
    }
}

#[cfg(test)]
mod tests;
