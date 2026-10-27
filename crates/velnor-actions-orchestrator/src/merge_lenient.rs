// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_actions_contract::{QualificationDispatch, RequiredJobResult, WorkflowEvent};

use super::MergeRequest;
use crate::cover::shard::{ResourceLimits, ShardProof};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LenientRequest {
    schema: u32,
    run_key: String,
    #[serde(default)]
    actual_event: Option<WorkflowEvent>,
    #[serde(default)]
    actual_qualification: Option<QualificationDispatch>,
    #[serde(default)]
    candidate_attestation: Option<serde_json::Value>,
    #[serde(default)]
    plan: Option<serde_json::Value>,
    #[serde(default)]
    matrix: Option<serde_json::Value>,
    matrix_reports: Vec<serde_json::Value>,
    #[serde(default)]
    task_reports: Vec<serde_json::Value>,
    #[serde(default)]
    check_proofs: Vec<serde_json::Value>,
    required_job_ids: Vec<String>,
    required_jobs: Vec<RequiredJobResult>,
    #[serde(default)]
    assembly_errors: Vec<String>,
    #[serde(default)]
    shard_proofs: Vec<ShardProof>,
    #[serde(default)]
    limits: Option<ResourceLimits>,
    #[serde(default)]
    reference_task_ids: Option<Vec<String>>,
}

pub(crate) fn lenient_request(envelope: &serde_json::Value) -> Option<MergeRequest> {
    let raw: LenientRequest = serde_json::from_value(envelope.clone()).ok()?;
    let mut assembly_errors = raw.assembly_errors;
    let candidate_attestation = untyped_option(
        raw.candidate_attestation,
        "unparsable_candidate_attestation",
        &mut assembly_errors,
    );
    let plan = untyped_option(raw.plan, "unparsable_plan", &mut assembly_errors);
    let matrix = untyped_option(raw.matrix, "unparsable_matrix", &mut assembly_errors);
    let matrix_reports = untyped_list(
        raw.matrix_reports,
        "unparsable_matrix_report",
        &mut assembly_errors,
    );
    let task_reports = untyped_list(
        raw.task_reports,
        "unparsable_task_report",
        &mut assembly_errors,
    );
    Some(MergeRequest {
        schema: raw.schema,
        run_key: raw.run_key,
        actual_event: raw.actual_event,
        actual_qualification: raw.actual_qualification,
        candidate_attestation,
        plan,
        matrix,
        matrix_reports,
        task_reports,
        check_proofs: raw.check_proofs,
        required_job_ids: raw.required_job_ids,
        required_jobs: raw.required_jobs,
        group_ledgers: Vec::new(),
        assembly_errors,
        shard_proofs: raw.shard_proofs,
        limits: raw.limits,
        reference_task_ids: raw.reference_task_ids,
    })
}

fn untyped_option<T>(
    value: Option<serde_json::Value>,
    kind: &str,
    errors: &mut Vec<String>,
) -> Option<T>
where
    T: serde::de::DeserializeOwned,
{
    let value = value?;
    if let Ok(item) = serde_json::from_value(value) {
        Some(item)
    } else {
        errors.push(kind.to_owned());
        None
    }
}

fn untyped_list<T>(values: Vec<serde_json::Value>, kind: &str, errors: &mut Vec<String>) -> Vec<T>
where
    T: serde::de::DeserializeOwned,
{
    values
        .into_iter()
        .filter_map(|value| {
            if let Ok(item) = serde_json::from_value(value) {
                Some(item)
            } else {
                errors.push(kind.to_owned());
                None
            }
        })
        .collect()
}
