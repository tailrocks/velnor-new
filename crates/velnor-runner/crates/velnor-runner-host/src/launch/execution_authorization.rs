//! Hook facts and source identities for one execution decision.

mod request;
mod source;
mod verifier;
mod wire;

pub(crate) use request::ExecutionSourceRequest;
#[cfg(test)]
pub(crate) use source::test_authorized_execution_source;
pub(crate) use source::{AuthorizedExecutionSource, PersistedExecutionSource};
pub(crate) use verifier::verify_execution_source;
pub(crate) use wire::{
    ExecutionProofChallenge, ExecutionProofChallengeFields, PersistedExecutionDecision,
};

use sha2::{Digest, Sha256};

const MAX_REQUEST_BYTES: usize = 16 * 1024;
const MAX_CLOSURE_FILES: usize = 32;
pub(crate) const MAX_PROOF_BYTES: usize = 128 * 1024;

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn optional_text(value: Option<&str>, maximum: usize) -> bool {
    value.is_none_or(|text| valid_text(text, maximum))
}

fn optional_git_sha(value: Option<&str>) -> bool {
    value.is_none_or(valid_git_sha)
}

fn valid_relative_path(value: &str) -> bool {
    valid_text(value, 2048)
        && !value.starts_with('/')
        && !value
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
}

fn valid_git_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_object_id(value: &str) -> bool {
    (12..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn valid_deny_code(value: &str) -> bool {
    matches!(
        value,
        "source_mismatch"
            | "workflow_not_approved"
            | "policy_changed"
            | "proof_malformed"
            | "interrupted_after_restart"
            | "association_mismatch"
            | "runner_terminated"
    )
}

fn digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
