//! Receipt-scoped branch and immutable identity parsing.

use serde_json::Value;

use crate::{OrchestratorError, internal::internal};

use super::super::candidate::expected_branch;

pub(super) fn branch_prefix(receipt_digest: &str) -> Result<String, OrchestratorError> {
    let digest = receipt_digest
        .strip_prefix("b3-")
        .filter(|value| is_lower_hex(value, 64))
        .ok_or_else(|| internal("tap_merge_history_receipt_digest"))?;
    Ok(format!("velnor/preview/{digest}-"))
}

pub(super) fn belongs_to_receipt(
    branch: &str,
    body: &str,
    receipt_branch_prefix: &str,
    digest_marker: &str,
) -> Result<bool, OrchestratorError> {
    if !branch.starts_with(receipt_branch_prefix) {
        // Receipt markers are public and may be copied into unrelated PRs. Only
        // the deterministic branch namespace establishes ownership.
        return Ok(false);
    }
    if !body.contains(digest_marker) {
        return Err(internal("tap_merge_history_marker_conflict"));
    }
    Ok(true)
}

pub(super) fn branch_base(receipt_digest: &str, branch: &str) -> Result<String, OrchestratorError> {
    let prefix = branch_prefix(receipt_digest)?;
    let base = branch
        .strip_prefix(&prefix)
        .filter(|value| is_lower_hex(value, 40))
        .ok_or_else(|| internal("tap_merge_history_branch_invalid"))?;
    if branch != expected_branch(receipt_digest, base)? {
        return Err(internal("tap_merge_history_branch_invalid"));
    }
    Ok(base.to_owned())
}

pub(super) fn required_text<'a>(
    value: &'a Value,
    pointer: &str,
    problem: &str,
) -> Result<&'a str, OrchestratorError> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(problem))
}

pub(super) fn required_sha(
    value: &Value,
    pointer: &str,
    problem: &str,
) -> Result<String, OrchestratorError> {
    let sha = required_text(value, pointer, problem)?;
    if !is_lower_hex(sha, 40) {
        return Err(internal(problem));
    }
    Ok(sha.to_owned())
}

pub(super) fn verify_repo_id(
    value: Option<&Value>,
    expected: u64,
) -> Result<(), OrchestratorError> {
    if value.and_then(Value::as_u64) != Some(expected) {
        return Err(internal("tap_merge_history_repository_id"));
    }
    Ok(())
}

pub(super) fn validate_observed_base(
    pull: &Value,
    original_base_sha: &str,
    merged: bool,
) -> Result<(), OrchestratorError> {
    let state = pull["state"].as_str();
    let observed = required_sha(pull, "/base/sha", "tap_merge_history_base")?;
    if (merged || state == Some("open")) && observed != original_base_sha {
        return Err(internal("tap_merge_history_base_binding"));
    }
    Ok(())
}

pub(super) fn commit_matches(
    commit: &Value,
    head_sha: &str,
    base_sha: &str,
    message: &str,
) -> bool {
    let Some(parents) = commit["parents"].as_array() else {
        return false;
    };
    commit["sha"].as_str() == Some(head_sha)
        && commit["message"].as_str() == Some(message)
        && parents.len() == 1
        && parents[0]["sha"].as_str() == Some(base_sha)
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
#[path = "tap_merge_history_identity_tests.rs"]
mod tests;
