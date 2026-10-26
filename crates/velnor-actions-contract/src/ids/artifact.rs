// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::errors::ContractError;
use crate::ids::{is_component_byte, validate_matrix_key, validate_run_key};

pub fn artifact_id_for_plan(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    let id = format!("velnor-plan-{run_key}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

pub fn artifact_id_for_matrix(run_key: &str, matrix_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    validate_matrix_key(matrix_key)?;
    let id = format!("velnor-matrix-{run_key}-{matrix_key}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

pub fn artifact_id_for_final(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    let id = format!("velnor-final-{run_key}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

pub fn artifact_id_for_crate_job(run_key: &str, job_id: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    crate::workflow::jobs::validate_job_id(job_id)
        .map_err(|_| ContractError::identity("artifact_id", "bad_job_artifact"))?;
    let id = format!("velnor-crate-{run_key}-{job_id}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

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
    Err(ContractError::identity(
        "artifact_id",
        "unknown_artifact_kind",
    ))
}

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

fn validate_job_artifact(rest: &str) -> Result<(), ContractError> {
    for (index, _) in rest.match_indices('-') {
        let head = &rest[..index];
        let tail = &rest[index + 1..];
        if validate_run_key(head).is_ok() && crate::workflow::jobs::validate_job_id(tail).is_ok() {
            return Ok(());
        }
    }
    Err(ContractError::identity("artifact_id", "bad_job_artifact"))
}

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

fn is_target_key(tail: &str) -> bool {
    !tail.is_empty()
        && tail != "."
        && tail != ".."
        && !tail.starts_with('-')
        && !tail.ends_with('-')
        && !tail.contains("--")
        && tail.bytes().all(is_component_byte)
}
