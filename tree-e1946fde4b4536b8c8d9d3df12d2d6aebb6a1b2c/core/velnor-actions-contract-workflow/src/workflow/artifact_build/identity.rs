//! Stable identity formatting and validation for artifact results.

use serde::Serialize;
use velnor_actions_contract::canonical::{canonical_json_bytes, digest_b3, validate_digest};
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract::ids::validate_run_key;

use super::{ArtifactBuildIdentity, ArtifactBuildProvider, validate_repository_identity};

/// Deterministic artifact name scoped by run, attempt, provider, and task.
/// # Errors
pub fn artifact_name(identity: &ArtifactBuildIdentity) -> Result<String, ContractError> {
    validate_identity(identity)?;
    artifact_name_for(
        &identity.run_id,
        identity.run_attempt,
        identity.provider,
        &identity.task_id,
    )
}

/// Deterministic artifact name used in the plan matrix and upload action.
/// # Errors
pub fn artifact_name_for(
    run_id: &str,
    run_attempt: u32,
    provider: ArtifactBuildProvider,
    task_id: &str,
) -> Result<String, ContractError> {
    let parsed_run_id = run_id.parse::<u64>().ok();
    if parsed_run_id.is_none_or(|value| value == 0 || value.to_string() != run_id) {
        return Err(ContractError::identity("artifact.run_id", "bad_run_id"));
    }
    if run_attempt == 0 {
        return Err(ContractError::identity(
            "artifact.run_attempt",
            "must_be_positive",
        ));
    }
    if !velnor_actions_contract_config::is_valid_verification_task_id(task_id) {
        return Err(ContractError::identity("artifact.task_id", "bad_task_id"));
    }
    Ok(format!(
        "velnor-build-{}-{}-{}-{}",
        run_id,
        run_attempt,
        provider.token(),
        task_id
    ))
}

/// Derive the same name directly from the validated plan run key.
/// # Errors
pub fn artifact_name_for_run_key(
    run_key: &str,
    provider: ArtifactBuildProvider,
    task_id: &str,
) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    let run = run_key
        .strip_prefix('r')
        .ok_or_else(|| ContractError::identity("artifact.run_key", "bad_run_key"))?;
    let (run_id, attempt) = run
        .split_once("-a")
        .ok_or_else(|| ContractError::identity("artifact.run_key", "bad_run_key"))?;
    let run_attempt = attempt
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0 && value.to_string() == attempt)
        .ok_or_else(|| ContractError::identity("artifact.run_attempt", "bad_run_attempt"))?;
    artifact_name_for(run_id, run_attempt, provider, task_id)
}

/// Hash a canonical plan before it is copied into artifact identities.
/// # Errors
pub fn canonical_plan_digest<T: Serialize>(plan: &T) -> Result<String, ContractError> {
    Ok(digest_b3(&canonical_json_bytes(plan)?))
}

pub(super) fn validate_identity(identity: &ArtifactBuildIdentity) -> Result<(), ContractError> {
    validate_repository_identity(&identity.repository_id, &identity.repository)?;
    if !is_commit_sha(&identity.source_sha) {
        return Err(ContractError::identity(
            "artifact.source_sha",
            "bad_commit_sha",
        ));
    }
    validate_digest(&identity.plan_digest)?;
    let parsed_run_id = identity.run_id.parse::<u64>().ok();
    if parsed_run_id.is_none_or(|value| value == 0 || value.to_string() != identity.run_id) {
        return Err(ContractError::identity("artifact.run_id", "bad_run_id"));
    }
    if identity.run_attempt == 0 {
        return Err(ContractError::identity(
            "artifact.run_attempt",
            "must_be_positive",
        ));
    }
    let job_id_is_valid = match identity.provider {
        ArtifactBuildProvider::GithubHosted => matches!(
            identity.workflow_job_id.as_str(),
            "artifact-build" | "artifact-build__hosted"
        ),
        ArtifactBuildProvider::VelnorScaleSet => matches!(
            identity.workflow_job_id.as_str(),
            "artifact-build" | "artifact-build__local"
        ),
    };
    if !job_id_is_valid {
        return Err(ContractError::identity(
            "artifact.workflow_job_id",
            "job_key_provider_mismatch",
        ));
    }
    if !velnor_actions_contract_config::is_valid_verification_task_id(&identity.task_id) {
        return Err(ContractError::identity("artifact.task_id", "bad_task_id"));
    }
    Ok(())
}

fn is_commit_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
