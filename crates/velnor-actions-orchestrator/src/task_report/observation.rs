// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    Digest, MatrixEntry, MatrixReport, Plan, TaskReport, canonical_json_bytes, digest_b3,
    parse_strict_json, validate_artifact_id, validate_job_id, validate_matrix_key,
    validate_task_report_id,
};

use crate::OrchestratorError;
use crate::internal::internal_contract;

pub(crate) const UNIT_OBSERVATION_ENV: &str = "VELNOR_UNIT_OBSERVATION_JSON";
pub(crate) const UNIT_OBSERVATION_SCHEMA: u32 = 1;
pub(crate) const UNIT_OBSERVATION_SUFFIX: &str = ".observation.json";
pub(crate) const MAX_UNIT_OBSERVATION_BYTES: u64 = 64 << 10;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnitObservationBinding {
    pub(crate) schema: u32,
    pub(crate) task_report_id: String,
    pub(crate) artifact_id: String,
    pub(crate) member_name: String,
    pub(crate) member_size_bytes: u64,
    pub(crate) member_digest: String,
    pub(crate) producer_job: String,
    pub(crate) producer_attempt: u64,
    pub(crate) source_sha: String,
    pub(crate) observation_session_id: String,
    pub(crate) unit_ledger_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnitObservationExpectation {
    pub(crate) task_report_id: String,
    pub(crate) artifact_id: String,
    pub(crate) member_name: String,
    pub(crate) producer_job: String,
    pub(crate) producer_attempt: Option<u64>,
    pub(crate) source_sha: String,
}

fn observation_identity(
    field: &'static str,
    problem: String,
) -> velnor_actions_contract::ContractError {
    velnor_actions_contract::ContractError::identity(field, problem)
}

fn is_source_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn check_member_name(value: &str) -> Result<(), velnor_actions_contract::ContractError> {
    if value.is_empty() || value.starts_with('/') || value.contains('\\') || value.contains('\0') {
        return Err(observation_identity(
            "unit_observation.member_name",
            "non_relative_member".to_owned(),
        ));
    }
    if value
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(observation_identity(
            "unit_observation.member_name",
            "escaping_member".to_owned(),
        ));
    }
    Ok(())
}

fn check_session_id(value: &str) -> Result<(), velnor_actions_contract::ContractError> {
    if value.is_empty()
        || value.contains('\0')
        || value.contains('/')
        || value.contains('\\')
        || value.contains("..")
    {
        return Err(observation_identity(
            "unit_observation.observation_session_id",
            "bad_session".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn run_attempt(run_key: &str) -> Option<u64> {
    if run_key == "local" {
        return None;
    }
    let rest = run_key.strip_prefix('r')?;
    let (run, attempt) = rest.split_once("-a")?;
    if run.is_empty()
        || attempt.is_empty()
        || !run.bytes().all(|byte| byte.is_ascii_digit())
        || !attempt.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    attempt.parse::<u64>().ok()
}

pub(crate) fn unit_observation_member_name(
    matrix_key: &str,
    task_report_id: &str,
) -> Result<String, velnor_actions_contract::ContractError> {
    validate_matrix_key(matrix_key).map_err(|_| {
        observation_identity("unit_observation.member_name", "bad_matrix_key".to_owned())
    })?;
    validate_task_report_id(task_report_id).map_err(|_| {
        observation_identity(
            "unit_observation.member_name",
            "bad_task_report_id".to_owned(),
        )
    })?;
    Ok(format!(
        "{matrix_key}/tasks/{task_report_id}{UNIT_OBSERVATION_SUFFIX}"
    ))
}

pub(crate) fn parse_unit_observation_binding(
    text: &str,
) -> Result<UnitObservationBinding, velnor_actions_contract::ContractError> {
    if text.len() as u64 > MAX_UNIT_OBSERVATION_BYTES {
        return Err(observation_identity(
            "unit_observation",
            "oversize_observation".to_owned(),
        ));
    }
    let value = parse_strict_json(text).map_err(|_| {
        observation_identity("unit_observation", "unparsable_observation".to_owned())
    })?;
    let binding: UnitObservationBinding = serde_json::from_value(value).map_err(|_| {
        observation_identity("unit_observation", "unparsable_observation".to_owned())
    })?;
    binding.validate_shape()?;
    Ok(binding)
}

fn observation_content_bytes(
    binding: &UnitObservationBinding,
) -> Result<Vec<u8>, velnor_actions_contract::ContractError> {
    let content = serde_json::json!({
        "schema": binding.schema,
        "task_report_id": binding.task_report_id,
        "artifact_id": binding.artifact_id,
        "member_name": binding.member_name,
        "producer_job": binding.producer_job,
        "producer_attempt": binding.producer_attempt,
        "source_sha": binding.source_sha,
        "observation_session_id": binding.observation_session_id,
        "unit_ledger_digest": binding.unit_ledger_digest,
    });
    canonical_json_bytes(&content)
}

impl UnitObservationBinding {
    fn validate_shape(&self) -> Result<(), velnor_actions_contract::ContractError> {
        if self.schema != UNIT_OBSERVATION_SCHEMA {
            return Err(observation_identity(
                "unit_observation.schema",
                "unsupported_observation_schema".to_owned(),
            ));
        }
        validate_task_report_id(&self.task_report_id).map_err(|_| {
            observation_identity(
                "unit_observation.task_report_id",
                "bad_task_report_id".to_owned(),
            )
        })?;
        validate_artifact_id(&self.artifact_id).map_err(|_| {
            observation_identity("unit_observation.artifact_id", "bad_artifact_id".to_owned())
        })?;
        check_member_name(&self.member_name)?;
        if self.member_size_bytes == 0 || self.member_size_bytes > MAX_UNIT_OBSERVATION_BYTES {
            return Err(observation_identity(
                "unit_observation.member_size_bytes",
                "bad_member_size".to_owned(),
            ));
        }
        Digest::parse(&self.member_digest).map_err(|_| {
            observation_identity(
                "unit_observation.member_digest",
                "bad_member_digest".to_owned(),
            )
        })?;
        validate_job_id(&self.producer_job).map_err(|_| {
            observation_identity(
                "unit_observation.producer_job",
                "bad_producer_job".to_owned(),
            )
        })?;
        if self.producer_attempt == 0 {
            return Err(observation_identity(
                "unit_observation.producer_attempt",
                "bad_producer_attempt".to_owned(),
            ));
        }
        if !is_source_sha(&self.source_sha) {
            return Err(observation_identity(
                "unit_observation.source_sha",
                "bad_source_sha".to_owned(),
            ));
        }
        check_session_id(&self.observation_session_id)?;
        Digest::parse(&self.unit_ledger_digest).map_err(|_| {
            observation_identity(
                "unit_observation.unit_ledger_digest",
                "bad_ledger_digest".to_owned(),
            )
        })?;
        Ok(())
    }
}

pub(crate) fn verify_unit_observation_binding(
    binding: &UnitObservationBinding,
    expectation: &UnitObservationExpectation,
) -> Result<(), velnor_actions_contract::ContractError> {
    binding.validate_shape()?;
    let mismatch = |_| {
        observation_identity(
            "unit_observation",
            "observation_binding_mismatch".to_owned(),
        )
    };
    if binding.task_report_id != expectation.task_report_id
        || binding.artifact_id != expectation.artifact_id
        || binding.member_name != expectation.member_name
        || binding.producer_job != expectation.producer_job
        || binding.source_sha != expectation.source_sha
    {
        return Err(mismatch("binding"));
    }
    if expectation
        .producer_attempt
        .is_some_and(|attempt| attempt != binding.producer_attempt)
    {
        return Err(mismatch("attempt"));
    }
    let content = observation_content_bytes(binding)?;
    if content.len() as u64 != binding.member_size_bytes
        || digest_b3(&content) != binding.member_digest
    {
        return Err(observation_identity(
            "unit_observation",
            "observation_digest_mismatch".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn stage_unit_observation_to(
    runner_temp: &Path,
    plan: &Plan,
    entry: &MatrixEntry,
    task_report_id: &str,
    observation_json: &str,
) -> Result<PathBuf, OrchestratorError> {
    let binding = parse_unit_observation_binding(observation_json).map_err(internal_contract)?;
    let member = unit_observation_member_name(&entry.matrix_key, task_report_id)
        .map_err(internal_contract)?;
    let expectation = UnitObservationExpectation {
        task_report_id: task_report_id.to_owned(),
        artifact_id: entry.artifact_id.clone(),
        member_name: member,
        producer_job: entry.job_id.clone(),
        producer_attempt: run_attempt(&plan.run_key),
        source_sha: plan.head.clone(),
    };
    verify_unit_observation_binding(&binding, &expectation).map_err(internal_contract)?;
    let bytes = canonical_json_bytes(&binding).map_err(internal_contract)?;
    let dir = runner_temp
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key)
        .join("tasks");
    crate::exclusive_write::create_dir_no_symlink(runner_temp, &dir)?;
    let path = dir.join(format!("{task_report_id}{UNIT_OBSERVATION_SUFFIX}"));
    crate::exclusive_write::write_exclusive(&path, &bytes, "unit_observation")?;
    Ok(path)
}

pub(crate) fn write_entry_reports(
    runner_temp: &Path,
    plan: &Plan,
    entry: &MatrixEntry,
    task: &TaskReport,
    matrix: &MatrixReport,
) -> Result<(), OrchestratorError> {
    let dir = runner_temp
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key);
    let task_files_dir = dir.join("tasks");
    crate::exclusive_write::create_dir_no_symlink(runner_temp, &task_files_dir)?;
    let matrix_bytes = canonical_json_bytes(matrix).map_err(internal_contract)?;
    let task_bytes = canonical_json_bytes(task).map_err(internal_contract)?;
    crate::exclusive_write::write_exclusive(
        &dir.join("matrix-report.json"),
        &matrix_bytes,
        "report",
    )?;
    crate::exclusive_write::write_exclusive(
        &task_files_dir.join(format!("{}.json", task.task_report_id)),
        &task_bytes,
        "report",
    )?;
    Ok(())
}
