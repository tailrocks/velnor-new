//! Derived GitHub artifact names and target keys.
use crate::errors::ContractError;
use crate::ids::{is_component_byte, validate_matrix_key, validate_run_key};

/// Derive the plan artifact name `velnor-plan-<run-key>`.
/// # Errors
pub fn artifact_id_for_plan(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    Ok(format!("velnor-plan-{run_key}"))
}

/// Derive the matrix artifact name `velnor-matrix-<run-key>-<matrix-key>`.
/// # Errors
pub fn artifact_id_for_matrix(run_key: &str, matrix_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    validate_matrix_key(matrix_key)?;
    Ok(format!("velnor-matrix-{run_key}-{matrix_key}"))
}

/// Derive the final artifact name `velnor-final-<run-key>`.
/// # Errors
pub fn artifact_id_for_final(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    Ok(format!("velnor-final-{run_key}"))
}

/// Derive `velnor-baseline-<commit>-<compat>` with full IDs (par §5).
/// # Errors
pub fn artifact_id_for_baseline(commit: &str, compat: &str) -> Result<String, ContractError> {
    if commit.len() != 40 || !is_lower_hex(commit) {
        return Err(ContractError::identity("artifact_id", "bad_source_commit"));
    }
    crate::canonical::validate_digest(compat)
        .map_err(|_| ContractError::identity("artifact_id", "bad_compatibility_id"))?;
    Ok(format!("velnor-baseline-{commit}-{compat}"))
}

/// Derive `velnor-candidate-<run-key>-<target-key>` from a target triple.
/// # Errors
pub fn artifact_id_for_candidate(run_key: &str, target: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    let key = target_key(target)?;
    Ok(format!("velnor-candidate-{run_key}-{key}"))
}

/// Validate a derived artifact name (plan/matrix/final/candidate).
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
    Ok(key)
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
    if commit.len() != 40 || !is_lower_hex(commit) {
        return Err(bad());
    }
    crate::canonical::validate_digest(compat).map_err(|_| bad())
}

/// Check lowercase hex.
fn is_lower_hex(text: &str) -> bool {
    text.bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Check target-key shape (matches [`target_key`] output grammar).
fn is_target_key(tail: &str) -> bool {
    !tail.is_empty()
        && !tail.starts_with('-')
        && !tail.ends_with('-')
        && !tail.contains("--")
        && tail.bytes().all(is_component_byte)
}
