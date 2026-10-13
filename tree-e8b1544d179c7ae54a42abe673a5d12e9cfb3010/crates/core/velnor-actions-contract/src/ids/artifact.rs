//! Derived GitHub artifact names and target keys.
use crate::errors::ContractError;
use crate::ids::{is_component_byte, validate_matrix_key, validate_run_key};

/// Derive the plan artifact name `velnor-plan-<run-key>`.
/// # Errors
pub fn artifact_id_for_plan(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    let id = format!("velnor-plan-{run_key}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

/// Derive the matrix artifact name `velnor-matrix-<run-key>-<matrix-key>`.
/// # Errors
pub fn artifact_id_for_matrix(run_key: &str, matrix_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    validate_matrix_key(matrix_key)?;
    let id = format!("velnor-matrix-{run_key}-{matrix_key}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

/// Derive the final artifact name `velnor-final-<run-key>`.
/// # Errors
pub fn artifact_id_for_final(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    let id = format!("velnor-final-{run_key}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

/// Derive one job's report artifact `velnor-crate-<run-key>-<job-id>`.
///
/// Each matrix job uploads exactly one artifact carrying every entry's
/// matrix report plus task reports (implementation-plan contract); the
/// job ID is the stable crate-job (or plan-job) ID, validated by the
/// job-ID grammar so the name round-trips through validation.
/// # Errors
pub fn artifact_id_for_crate_job(run_key: &str, job_id: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    super::job_ids::validate_job_id(job_id)
        .map_err(|_| ContractError::identity("artifact_id", "bad_job_artifact"))?;
    let id = format!("velnor-crate-{run_key}-{job_id}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

/// Derive `velnor-baseline-<commit>-<compat>` with full IDs (par §5).
/// # Errors
pub fn artifact_id_for_baseline(commit: &str, compat: &str) -> Result<String, ContractError> {
    if !super::is_lower_hex_len(commit, 40) {
        return Err(ContractError::identity("artifact_id", "bad_source_commit"));
    }
    crate::canonical::validate_digest(compat)
        .map_err(|_| ContractError::identity("artifact_id", "bad_compatibility_id"))?;
    let id = format!("velnor-baseline-{commit}-{compat}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

/// Validate a target-key shape (matches [`target_key`] output grammar).
/// # Errors
pub fn validate_target_key(value: &str) -> Result<(), ContractError> {
    if is_target_key(value) {
        Ok(())
    } else {
        Err(ContractError::identity(
            "target_key",
            "malformed_target_key",
        ))
    }
}

/// Validate a derived artifact name (plan/matrix/final/crate/candidate).
/// # Errors
pub fn validate_artifact_id(value: &str) -> Result<(), ContractError> {
    if let Some(run_key) = value.strip_prefix("velnor-plan-") {
        return validate_run_key(run_key)
            .map_err(|_| ContractError::identity("artifact_id", "bad_plan_artifact"));
    }
    if let Some(run_key) = value.strip_prefix("velnor-final-") {
        return validate_run_key(run_key)
            .map_err(|_| ContractError::identity("artifact_id", "bad_final_artifact"));
    }
    if let Some(rest) = value.strip_prefix("velnor-matrix-") {
        let Some((run_key, hex)) = rest.rsplit_once("-m-") else {
            return Err(ContractError::identity(
                "artifact_id",
                "bad_matrix_artifact",
            ));
        };
        validate_run_key(run_key)
            .map_err(|_| ContractError::identity("artifact_id", "bad_matrix_artifact"))?;
        return validate_matrix_key(&format!("m-{hex}"))
            .map_err(|_| ContractError::identity("artifact_id", "bad_matrix_artifact"));
    }
    if let Some(rest) = value.strip_prefix("velnor-crate-") {
        return validate_job_artifact(rest);
    }
    if let Some(rest) = value.strip_prefix("velnor-candidate-") {
        return validate_candidate_artifact(rest);
    }
    if let Some(rest) = value.strip_prefix("velnor-baseline-") {
        return validate_baseline_artifact(rest);
    }
    Err(ContractError::identity(
        "artifact_id",
        "unknown_artifact_kind",
    ))
}

/// Convert a target triple to a target key (lowercase, `-` runs collapsed).
/// # Errors
pub fn target_key(target: &str) -> Result<String, ContractError> {
    if target.is_empty() {
        return Err(ContractError::identity("target_key", "empty_target"));
    }
    let mut key = String::with_capacity(target.len());
    let mut dash_pending = true;
    for byte in target.bytes() {
        if byte.is_ascii_alphanumeric() {
            key.push(byte.to_ascii_lowercase() as char);
            dash_pending = false;
        } else if !dash_pending {
            key.push('-');
            dash_pending = true;
        }
    }
    while key.ends_with('-') {
        key.pop();
    }
    if key.is_empty() {
        return Err(ContractError::identity("target_key", "empty_target_key"));
    }
    Ok(super::TargetKey::parse(&key)?.into_inner())
}

/// Validate the run-key/job-ID tail of a crate-job artifact name.
///
/// Both halves admit `-`, so every split is tried (same approach as
/// the candidate tail): the name validates when some split yields a
/// valid run key plus a valid job ID.
fn validate_job_artifact(rest: &str) -> Result<(), ContractError> {
    for (index, _) in rest.match_indices('-') {
        let head = &rest[..index];
        let tail = &rest[index + 1..];
        if validate_run_key(head).is_ok() && super::job_ids::validate_job_id(tail).is_ok() {
            return Ok(());
        }
    }
    Err(ContractError::identity("artifact_id", "bad_job_artifact"))
}

/// Validate the run-key/target-key tail of a candidate artifact name.
fn validate_candidate_artifact(rest: &str) -> Result<(), ContractError> {
    for (index, _) in rest.match_indices('-') {
        let head = &rest[..index];
        let tail = &rest[index + 1..];
        if validate_run_key(head).is_ok() && is_target_key(tail) {
            return Ok(());
        }
    }
    Err(ContractError::identity(
        "artifact_id",
        "bad_candidate_artifact",
    ))
}

/// Validate the commit/compat tail of a baseline artifact name.
fn validate_baseline_artifact(rest: &str) -> Result<(), ContractError> {
    let bad = || ContractError::identity("artifact_id", "bad_baseline_artifact");
    let Some((commit, compat)) = rest.split_once('-') else {
        return Err(bad());
    };
    if !super::is_lower_hex_len(commit, 40) {
        return Err(bad());
    }
    crate::canonical::validate_digest(compat).map_err(|_| bad())
}

/// Check target-key shape (matches [`target_key`] output grammar).
fn is_target_key(tail: &str) -> bool {
    !tail.is_empty()
        && tail != "."
        && tail != ".."
        && !tail.starts_with('-')
        && !tail.ends_with('-')
        && !tail.contains("--")
        && tail.bytes().all(is_component_byte)
}

#[cfg(test)]
mod tests;
