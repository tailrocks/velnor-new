// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;

pub const ARTIFACT_ID_VARIABLE: &str = "VELNOR_EVIDENCE_ARTIFACT_ID";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_ID_LENGTH: usize = 16;

pub fn validate_artifact_id(id: &str) -> Result<(), CiError> {
    let canonical = id
        .as_bytes()
        .first()
        .is_some_and(|first| (b'1'..=b'9').contains(first))
        && id.bytes().all(|byte| byte.is_ascii_digit());
    if !canonical {
        return Err(CiError::new(format!(
            "artifact id {id:?} is not a positive decimal integer without leading zeros"
        )));
    }
    if id.len() > MAX_ID_LENGTH {
        return Err(CiError::new(format!(
            "artifact id {id} has more than {MAX_ID_LENGTH} digits"
        )));
    }
    match id.parse::<u64>() {
        Ok(value) if value <= MAX_SAFE_INTEGER => Ok(()),
        _ => Err(CiError::new(format!(
            "artifact id {id} exceeds {MAX_SAFE_INTEGER}"
        ))),
    }
}
