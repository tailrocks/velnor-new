use serde::Serialize;
use sha2::{Digest, Sha256};

use velnor_resource_probe::ProbeError;

use super::{Challenge, Decision, SourceRequest, context};

pub(super) fn validate_challenge(challenge: &Challenge) -> Result<(), ProbeError> {
    if challenge.schema_version != 1
        || challenge.reservation_id <= 0
        || challenge.intent_id <= 0
        || challenge.witness_request_id <= 0
        || challenge.runner_id <= 0
        || challenge.runner_scale_set_id <= 0
        || challenge.policy_generation == 0
    {
        return Err(ProbeError::Invalid("job_hook_challenge_identity"));
    }
    validate_hex(&challenge.nonce, 64, "job_hook_challenge")?;
    validate_hex(&challenge.policy_digest, 64, "job_hook_policy_digest")?;
    validate_hex(
        &challenge.docker_root_digest,
        64,
        "job_hook_docker_root_digest",
    )?;
    validate_hex(
        &challenge.instance_binding_digest,
        64,
        "job_hook_instance_binding_digest",
    )?;
    validate_hex(&challenge.channel_digest, 64, "job_hook_channel_digest")?;
    validate_container_id(&challenge.runner_container_id)?;
    validate_container_id(&challenge.dind_container_id)?;
    context::bounded_text(&challenge.runner_name, 128, "job_hook_runner_name")?;
    context::bounded_text(&challenge.engine_id, 128, "job_hook_engine_id")?;
    Ok(())
}

fn validate_container_id(value: &str) -> Result<(), ProbeError> {
    validate_hex(value, 64, "job_hook_container_id")
}

fn validate_hex(value: &str, length: usize, field: &'static str) -> Result<(), ProbeError> {
    if value.len() != length
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(ProbeError::Invalid(field));
    }
    Ok(())
}

pub(super) fn validate_decision(
    challenge: &Challenge,
    request: &SourceRequest,
    decision: &Decision,
    request_sha256: &str,
) -> Result<(), ProbeError> {
    if decision.schema_version != 1
        || !decision.allow
        || decision.challenge != challenge.nonce
        || decision.request_sha256 != request_sha256
        || decision.reservation_id != challenge.reservation_id
        || decision.intent_id != challenge.intent_id
        || decision.actual_request_id <= 0
        || decision.run_id != request.run_id
        || decision.run_attempt != request.run_attempt
        || decision.job_key != request.job_key
        || decision.runner_id != challenge.runner_id
        || decision.policy_generation != challenge.policy_generation
        || decision.policy_digest != challenge.policy_digest
        || decision
            .native_job_id
            .as_deref()
            .is_none_or(|value| bounded_identifier(value, 256).is_err())
        || decision
            .rest_job_id
            .as_deref()
            .is_none_or(|value| bounded_identifier(value, 256).is_err())
        || decision.deny_code.is_some()
    {
        return Err(ProbeError::Invalid("job_hook_decision_mismatch"));
    }
    validate_hex(&decision.request_sha256, 64, "job_hook_request_digest")?;
    Ok(())
}

fn bounded_identifier(value: &str, limit: usize) -> Result<(), ProbeError> {
    if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(ProbeError::Invalid("job_hook_decision_identifier"));
    }
    Ok(())
}

pub(super) fn canonical_json<T: Serialize>(
    value: &T,
    limit: usize,
    field: &'static str,
) -> Result<Vec<u8>, ProbeError> {
    let bytes = serde_json::to_vec(value).map_err(|_| ProbeError::Invalid(field))?;
    if bytes.len() > limit {
        return Err(ProbeError::InputTooLarge(field));
    }
    Ok(bytes)
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _write_result = write!(text, "{byte:02x}");
    }
    text
}
