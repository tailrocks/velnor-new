// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::canonical::canonical_json_bytes;
use crate::errors::ContractError;
use crate::strict_json::{MAX_UNTRUSTED_DOCUMENT_BYTES, parse_strict_json_bytes};
use crate::workflow::plan::{Plan, PlanMatrix};

pub const PLAN_JSON_FILENAME: &str = "plan.json";

pub const MATRIX_JSON_FILENAME: &str = "matrix.json";

pub const FINAL_JSON_FILENAME: &str = "final-report.json";

pub const CANDIDATE_ATTESTATION_FILENAME: &str = "candidate-attestation.json";

pub const CANDIDATE_EVIDENCE_SUBDIR: &str = "candidate";

pub fn plan_json_bytes(plan: &Plan) -> Result<Vec<u8>, ContractError> {
    plan.validate()?;
    canonical_json_bytes(plan)
}

pub fn matrix_json_bytes(matrix: &PlanMatrix) -> Result<Vec<u8>, ContractError> {
    canonical_json_bytes(matrix)
}

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
