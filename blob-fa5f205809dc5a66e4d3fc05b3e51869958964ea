//! Bounded receipt errors, digests and parser guards.

use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::errors::ContractError;
use crate::workflow::QualificationRunRef;

use super::types::{MAX_JSON_NESTING, MAX_RECEIPT_TEXT_BYTES, QualificationCacheReceipt};

pub(super) fn receipt_digest(receipt: &QualificationCacheReceipt) -> Result<String, ContractError> {
    Ok(digest_b3(&canonical_json_bytes(receipt)?))
}

pub(super) fn check_json_nesting(bytes: &[u8]) -> Result<(), ContractError> {
    let (mut depth, mut quoted, mut escaped) = (0_usize, false, false);
    for byte in bytes {
        if quoted {
            match (escaped, byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => quoted = false,
                _ => {}
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > MAX_JSON_NESTING {
                        return Err(invalid("json_too_deep"));
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(())
}

pub(super) fn check_text(value: &str) -> Result<(), ContractError> {
    if value.is_empty()
        || value.len() > MAX_RECEIPT_TEXT_BYTES
        || value.chars().any(char::is_control)
    {
        Err(invalid("receipt_text_invalid"))
    } else {
        Ok(())
    }
}

pub(super) fn invalid(reason: &str) -> ContractError {
    ContractError::identity("qualification.cache_receipt", reason)
}

pub(super) fn run_ref_before(previous: QualificationRunRef, current: QualificationRunRef) -> bool {
    (previous.run_id, previous.run_attempt) < (current.run_id, current.run_attempt)
}
