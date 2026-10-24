//! Read and compare the exact successful reports for one Qualification lane.

use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;
use velnor_actions_contract::workflow::{
    MatrixReport, MatrixStatus, ObligationDecision, Plan, TaskReport, TaskStatus,
};
use velnor_actions_contract::{
    ExecuteTaskRef, canonical_json_bytes, digest_b3, parse_strict_json, report_id_for_matrix,
    task_report_id_for_task,
};

use super::types::{QualificationMatrixReportObservation, QualificationSourceReportObservation};
use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};

/// Report details for one lane in the source evidence file.
#[derive(Debug)]
pub(super) struct SuccessfulLaneReports {
    /// Report outcome read from the task file.
    pub(super) task: QualificationSourceReportObservation,
    /// Report outcome read from the matrix file.
    pub(super) matrix: QualificationMatrixReportObservation,
}

/// Read one task report and its matrix aggregate for a Qualification lane.
///
/// # Errors
///
/// If a report is absent or does not use the JSON format given by the contract, give an error.
/// If the report does not show success, give an error.
/// If the report does not agree with the plan entry or its one-task aggregate, give an error.
pub(super) fn read_successful_lane_reports(
    runner_temp: &Path,
    plan: &Plan,
    entry: &velnor_actions_contract::MatrixEntry,
    task_digest: &str,
) -> Result<SuccessfulLaneReports, OrchestratorError> {
    require_single_task_entry(entry)?;
    if entry.task_id.is_empty() || entry.task_digest != task_digest {
        return Err(internal("qualification_source_task_plan_mismatch"));
    }
    let obligation = plan
        .obligations
        .iter()
        .find(|obligation| obligation.task_id == entry.task_id)
        .ok_or_else(|| internal("qualification_source_obligation_missing"))?;
    if obligation.decision != ObligationDecision::Execute
        || obligation.task_digest != task_digest
        || obligation.input_digest != entry.input_digest
    {
        return Err(internal("qualification_source_obligation_mismatch"));
    }

    let expected_task_id = task_report_id_for_task(&plan.run_key, &entry.matrix_key, task_digest)
        .map_err(internal_contract)?;
    let matrix_path = runner_temp
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key);
    let task_path = matrix_path
        .join("tasks")
        .join(format!("{expected_task_id}.json"));
    let matrix_bytes = read_report(&matrix_path.join("matrix-report.json"))?;
    let task_bytes = read_report(&task_path)?;
    let task: TaskReport = parse_report(&task_bytes, "task")?;
    task.validate_for_plan_entry(entry)
        .map_err(internal_contract)?;
    task.validate_outputs_declared(&entry.declared_outputs)
        .map_err(internal_contract)?;
    if task.task_report_id != expected_task_id
        || task.run_key != plan.run_key
        || task.event != plan.event
        || task.trust != plan.trust
        || task.task_id != entry.task_id
        || task.task_digest != task_digest
        || task.status != TaskStatus::Executed
        || task.exit_code != 0
    {
        return Err(internal("qualification_source_task_report_mismatch"));
    }

    let matrix: MatrixReport = parse_report(&matrix_bytes, "matrix")?;
    matrix.validate().map_err(internal_contract)?;
    let expected_matrix =
        crate::task_report::single_task_aggregate(plan, entry, &task).map_err(internal_contract)?;
    let expected_bytes = canonical_json_bytes(&expected_matrix).map_err(internal_contract)?;
    if matrix_bytes != expected_bytes || matrix.status != MatrixStatus::Passed {
        return Err(internal("qualification_source_matrix_report_mismatch"));
    }
    let expected_matrix_id =
        report_id_for_matrix(&plan.run_key, &entry.matrix_key).map_err(internal_contract)?;
    if matrix.report_id != expected_matrix_id {
        return Err(internal("qualification_source_matrix_report_id"));
    }

    Ok(SuccessfulLaneReports {
        task: QualificationSourceReportObservation {
            report_id: task.task_report_id,
            digest_b3: digest_b3(&task_bytes),
            status: task.status,
            exit_code: task.exit_code,
        },
        matrix: QualificationMatrixReportObservation {
            report_id: matrix.report_id,
            digest_b3: digest_b3(&matrix_bytes),
            status: matrix.status,
        },
    })
}

fn require_single_task_entry(
    entry: &velnor_actions_contract::MatrixEntry,
) -> Result<(), OrchestratorError> {
    let mut tasks = Vec::new();
    for task in entry.execute_task_ids.tasks.values() {
        match task {
            ExecuteTaskRef::Single(task_id) => tasks.push(task_id.as_str()),
            ExecuteTaskRef::Shards(shards) => {
                tasks.extend(shards.iter().map(String::as_str));
            }
        }
    }
    if tasks.as_slice() != [entry.task_id.as_str()] {
        return Err(internal("qualification_source_entry_not_single_task"));
    }
    Ok(())
}

fn read_report(path: &Path) -> Result<Vec<u8>, OrchestratorError> {
    let bytes = crate::retrieve_reports::staged_reads::read_staged_bytes(
        path,
        crate::retrieve_reports::MAX_STAGED_REPORT_BYTES,
    )
    .map_err(|kind| internal(&format!("qualification_source_report_{kind}")))?;
    Ok(bytes)
}

fn parse_report<T: DeserializeOwned + Serialize>(
    bytes: &[u8],
    kind: &str,
) -> Result<T, OrchestratorError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| internal(&format!("qualification_source_{kind}_report_encoding")))?;
    let value = parse_strict_json(text)
        .map_err(|_| internal(&format!("qualification_source_{kind}_report_json")))?;
    let report: T = serde_json::from_value(value)
        .map_err(|_| internal(&format!("qualification_source_{kind}_report_shape")))?;
    let canonical = canonical_json_bytes(&report).map_err(internal_contract)?;
    if canonical != bytes {
        return Err(internal(&format!(
            "qualification_source_{kind}_report_not_canonical"
        )));
    }
    Ok(report)
}
