//! Plan-artifact bytes and matrix agreement (wf §4).
//!
//! The plan artifact MUST contain `plan.json` plus `matrix.json`
//! (`{"include":[...]}`); final MUST validate canonical byte agreement.
//! These pure helpers render the bytes and compare them; planners write
//! the files and final downloads them.

use crate::canonical::canonical_json_bytes;
use crate::errors::ContractError;
use crate::workflow::plan::{Plan, PlanMatrix};

/// Plan document filename inside the plan artifact.
pub const PLAN_JSON_FILENAME: &str = "plan.json";
/// Matrix document filename inside the plan artifact.
pub const MATRIX_JSON_FILENAME: &str = "matrix.json";

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
/// downloads still agree; any content difference fails closed.
/// # Errors
pub fn check_matrix_agreement(
    plan_matrix: &PlanMatrix,
    matrix_file_bytes: &[u8],
) -> Result<(), ContractError> {
    let expect = canonical_json_bytes(plan_matrix)?;
    let value: serde_json::Value = serde_json::from_slice(matrix_file_bytes)
        .map_err(|err| ContractError::identity("matrix.json", format!("malformed_json:{err}")))?;
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
