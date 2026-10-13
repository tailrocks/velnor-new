//! Machine-readable task and matrix reports.
use crate::canonical::validate_digest;
use crate::errors::ContractError;
use crate::ids::{
    matrix_key_for_id, task_report_id_for_task, validate_matrix_key, validate_report_id,
    validate_run_key, validate_task_id, validate_task_report_id,
};
use crate::workflow::plan::{Trust, WorkflowEvent};
use serde::{Deserialize, Serialize};
/// Per-task machine-readable report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskReport {
    /// Report schema version; must be 1.
    pub schema: u32,
    /// Derived task-report ID.
    pub task_report_id: String,
    /// Run key.
    pub run_key: String,
    /// Triggering event.
    pub event: WorkflowEvent,
    /// Trust scope.
    pub trust: Trust,
    /// Owning matrix ID (`matrix_id ≡ MatrixEntry.id`).
    pub matrix_id: String,
    /// Matrix key.
    pub matrix_key: String,
    /// Executed task ID.
    pub task_id: String,
    /// Task digest.
    pub task_digest: String,
    /// Task status.
    pub status: TaskStatus,
    /// Required when `not_selected`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_selected_reason: Option<NotSelectedReason>,
    /// Cache outcome.
    pub cache: CacheOutcome,
    /// Process exit code.
    pub exit_code: i32,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Declared outputs only.
    #[serde(default)]
    pub outputs: Vec<String>,
}
/// Task execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Result reused.
    Reused,
    /// Task executed.
    Executed,
    /// Valid empty shard partition.
    EmptyPartition,
    /// Not selected.
    NotSelected,
    /// Task failed.
    Failed,
    /// Task cancelled.
    Cancelled,
}
/// `not_selected` reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotSelectedReason {
    /// Upstream task failed.
    UpstreamFailed,
    /// Not in plan.
    NotInPlan,
    /// Unsupported.
    Unsupported,
    /// Cancelled by policy.
    CancelledByPolicy,
}
/// Cache outcome record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheOutcome {
    /// Cache layer.
    pub layer: CacheLayer,
    /// Full cache key used.
    pub key: String,
    /// Lookup result.
    pub result: CacheResult,
    /// Required when `miss`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub miss_reason: Option<String>,
}
/// Cache layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheLayer {
    /// Cargo sources layer.
    Sources,
    /// MBX objects layer.
    Mbx,
    /// Task-result layer.
    Task,
}
/// Cache lookup result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheResult {
    /// Entry hit.
    Hit,
    /// Entry missed.
    Miss,
    /// No lookup attempted.
    NotAttempted,
}
/// Per-matrix aggregate report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixReport {
    /// Report schema version; must be 1.
    pub schema: u32,
    /// Derived report ID.
    pub report_id: String,
    /// Run key.
    pub run_key: String,
    /// Aggregated matrix ID (`matrix_id ≡ MatrixEntry.id`).
    pub matrix_id: String,
    /// Matrix key.
    pub matrix_key: String,
    /// Aggregate status.
    pub status: MatrixStatus,
    /// Scheduled task IDs (sorted).
    pub expected_task_ids: Vec<String>,
    /// Task-report IDs (sorted).
    pub task_report_ids: Vec<String>,
    /// One entry per scheduled task (sorted by task ID).
    pub tasks: Vec<MatrixTaskEntry>,
    /// Scheduled count.
    pub selected: u32,
    /// Reused count.
    pub reused: u32,
    /// Executed count.
    pub executed: u32,
    /// Empty-partition count.
    pub empty_partition: u32,
    /// Not-selected count.
    pub not_selected: u32,
    /// Failed count.
    pub failed: u32,
    /// Cancelled count.
    pub cancelled: u32,
}
/// One task entry in a matrix report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixTaskEntry {
    /// Task-report ID.
    pub task_report_id: String,
    /// Task ID.
    pub task_id: String,
    /// Task status.
    pub status: TaskStatus,
    /// Process exit code.
    pub exit_code: i32,
}
/// Matrix aggregate status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatrixStatus {
    /// All tasks passed.
    Passed,
    /// A task failed.
    Failed,
    /// A task was cancelled.
    Cancelled,
    /// Entry never ran.
    NotRun,
}
impl TaskReport {
    /// Report schema version.
    pub const SCHEMA: u32 = 1;
    /// Validate schema, derived ID, digests, and status coherence.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema)?;
        validate_run_key(&self.run_key)?;
        validate_matrix_key(&self.matrix_key)?;
        check_matrix_id(&self.matrix_id, &self.matrix_key)?;
        validate_task_id(&self.task_id)?;
        validate_digest(&self.task_digest)?;
        let expect = task_report_id_for_task(&self.run_key, &self.matrix_key, &self.task_digest)?;
        if expect != self.task_report_id {
            return Err(ContractError::identity("task_report_id", "report_mismatch"));
        }
        validate_task_report_id(&self.task_report_id)?;
        let not_selected = self.status == TaskStatus::NotSelected;
        if not_selected != self.not_selected_reason.is_some() {
            return Err(ContractError::identity(
                "not_selected_reason",
                "reason_mismatch",
            ));
        }
        let miss = self.cache.result == CacheResult::Miss;
        if miss != self.cache.miss_reason.is_some() {
            return Err(ContractError::identity(
                "cache.miss_reason",
                "reason_mismatch",
            ));
        }
        if let Some(reason) = &self.cache.miss_reason {
            crate::cachekey::validate_miss_reason(reason)?;
        }
        if self.cache.key.len() > 512 {
            return Err(ContractError::identity("cache.key", "key_too_long"));
        }
        for output in &self.outputs {
            crate::canonical::normalize_posix_path(output)?;
        }
        Ok(())
    }
    /// Check `outputs` lists only declared task outputs (cache §3).
    /// # Errors
    pub fn validate_outputs_declared(&self, declared: &[String]) -> Result<(), ContractError> {
        for output in &self.outputs {
            if !declared.contains(output) {
                return Err(ContractError::identity("outputs", "undeclared_output"));
            }
        }
        Ok(())
    }
}
impl MatrixReport {
    /// Report schema version.
    pub const SCHEMA: u32 = 1;
    /// Validate schema, derived ID, coverage, sorting, and counts.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema)?;
        validate_run_key(&self.run_key)?;
        validate_matrix_key(&self.matrix_key)?;
        check_matrix_id(&self.matrix_id, &self.matrix_key)?;
        validate_report_id(&self.report_id)?;
        let expect = crate::ids::report_id_for_matrix(&self.run_key, &self.matrix_key)?;
        if expect != self.report_id {
            return Err(ContractError::identity("report_id", "report_mismatch"));
        }
        if !is_sorted(&self.expected_task_ids) || !is_sorted(&self.task_report_ids) {
            return Err(ContractError::identity("matrix_report", "must_be_sorted"));
        }
        if self
            .tasks
            .windows(2)
            .any(|pair| pair[0].task_id > pair[1].task_id)
        {
            return Err(ContractError::identity(
                "matrix_report.tasks",
                "must_be_sorted",
            ));
        }
        if self.tasks.len() != self.expected_task_ids.len() {
            return Err(ContractError::identity(
                "matrix_report.tasks",
                "coverage_mismatch",
            ));
        }
        let mut report_ids: Vec<&str> = self
            .tasks
            .iter()
            .map(|task| task.task_report_id.as_str())
            .collect();
        report_ids.sort_unstable();
        let mut expect_ids: Vec<&str> = self.task_report_ids.iter().map(String::as_str).collect();
        expect_ids.sort_unstable();
        if report_ids != expect_ids {
            return Err(ContractError::identity(
                "matrix_report.tasks",
                "id_mismatch",
            ));
        }
        for task in &self.tasks {
            validate_task_report_id(&task.task_report_id)?;
            validate_task_id(&task.task_id)?;
        }
        let parts = self.reused
            + self.executed
            + self.empty_partition
            + self.not_selected
            + self.failed
            + self.cancelled;
        if parts != self.selected || self.selected as usize != self.tasks.len() {
            return Err(ContractError::identity(
                "matrix_report.counts",
                "count_mismatch",
            ));
        }
        Ok(())
    }
}
/// Check a schema-1 version marker.
fn check_schema(schema: u32) -> Result<(), ContractError> {
    if schema != 1 {
        return Err(ContractError::UnsupportedSchema {
            field: "schema",
            found: schema.to_string(),
            expected: "1",
        });
    }
    Ok(())
}
/// Check a string list is sorted.
fn is_sorted(list: &[String]) -> bool {
    list.windows(2).all(|pair| pair[0] <= pair[1])
}

/// Check key derivation (`matrix_id ≡ MatrixEntry.id`).
fn check_matrix_id(matrix_id: &str, matrix_key: &str) -> Result<(), ContractError> {
    if matrix_key_for_id(matrix_id)? != matrix_key {
        return Err(ContractError::identity("matrix_id", "id_mismatch"));
    }
    Ok(())
}
