//! Single-task aggregate construction for `write-task-report-v1`.
//!
//! Split from `task_report` so the report module keeps the 400-line
//! gate; the aggregate shape is unchanged.

use velnor_actions_contract::{
    MatrixEntry, MatrixReport, MatrixStatus, MatrixTaskEntry, Plan, TaskReport, TaskStatus,
    report_id_for_matrix,
};

/// Validated single-task aggregate over one written task report.
///
/// # Errors
///
/// Returns [`ContractError`] for derivation or validation failures.
pub(crate) fn single_task_aggregate(
    plan: &Plan,
    entry: &MatrixEntry,
    task: &TaskReport,
) -> Result<MatrixReport, velnor_actions_contract::ContractError> {
    let mut aggregate = MatrixReport {
        schema: MatrixReport::SCHEMA,
        report_id: report_id_for_matrix(&plan.run_key, &entry.matrix_key)?,
        run_key: plan.run_key.clone(),
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        status: MatrixStatus::Passed,
        expected_task_ids: vec![task.task_id.clone()],
        task_report_ids: vec![task.task_report_id.clone()],
        tasks: vec![MatrixTaskEntry {
            task_report_id: task.task_report_id.clone(),
            task_id: task.task_id.clone(),
            status: task.status,
            exit_code: task.exit_code,
        }],
        selected: 1,
        reused: 0,
        executed: 0,
        empty_partition: 0,
        not_selected: 0,
        failed: 0,
        cancelled: 0,
    };
    match task.status {
        TaskStatus::Reused => aggregate.reused = 1,
        TaskStatus::Executed => aggregate.executed = 1,
        TaskStatus::EmptyPartition => aggregate.empty_partition = 1,
        TaskStatus::NotSelected => aggregate.not_selected = 1,
        TaskStatus::Failed => {
            aggregate.failed = 1;
            aggregate.status = MatrixStatus::Failed;
        }
        TaskStatus::Cancelled => {
            aggregate.cancelled = 1;
            aggregate.status = MatrixStatus::Cancelled;
        }
    }
    if aggregate.report_id != entry.report_id {
        return Err(velnor_actions_contract::ContractError::identity(
            "report_id",
            "report_mismatch",
        ));
    }
    aggregate.validate()?;
    Ok(aggregate)
}
