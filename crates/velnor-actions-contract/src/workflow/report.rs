//! Machine-readable task and matrix reports.
use crate::workflow::plan::WorkflowEvent;
use crate::workflow::platform::PlatformBinding;
use crate::workflow::trust::Trust;
use serde::{Deserialize, Serialize};
/// Per-task machine-readable report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReport {
    /// Report schema version; must be 3.
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
    /// Explicit link from the planned platform ID to runtime-observed facts.
    pub platform_binding: PlatformBinding,
    /// Process exit code.
    pub exit_code: i32,
    /// Measured wall time in milliseconds, when collected.
    ///
    /// Absent telemetry is `None`, never a fabricated zero: a missing
    /// measurement must not read as an instant task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Declared outputs only.
    #[serde(default)]
    pub outputs: Vec<String>,
    /// Assigned isolated lane, when scheduled by lane (par §9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<u32>,
    /// Queue the task waited in, when queued (par §9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<String>,
    /// Shard partition id, when sharded (par §9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition: Option<String>,
    /// Schedule reason, when placement was decided (par §9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Measured timing breakdown, when collected (par §9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<TaskTiming>,
}
/// Measured per-slot task timing in milliseconds (par §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskTiming {
    /// Queue wait before dispatch.
    pub queue_ms: u64,
    /// Runner provisioning.
    pub runner_ms: u64,
    /// Task-body wall time.
    pub task_ms: u64,
    /// Cache restore/save handling.
    pub cache_ms: u64,
    /// Preparation before the payload.
    pub prep_ms: u64,
    /// Artifact downloads.
    pub download_ms: u64,
    /// Compiler wall time.
    pub compiler_ms: u64,
    /// MBX object handling.
    pub mbx_ms: u64,
    /// Test execution proper.
    pub test_ms: u64,
    /// Lock waits.
    pub lock_wait_ms: u64,
}
impl TaskTiming {
    /// Sum of separately measured slots.
    #[must_use]
    pub fn accounted_total(&self) -> u64 {
        self.slots()
            .iter()
            .fold(0, |sum, slot| sum.saturating_add(*slot))
    }
    /// All slots in canonical order.
    #[must_use]
    pub fn slots(&self) -> [u64; 10] {
        [
            self.queue_ms,
            self.runner_ms,
            self.task_ms,
            self.cache_ms,
            self.prep_ms,
            self.download_ms,
            self.compiler_ms,
            self.mbx_ms,
            self.test_ms,
            self.lock_wait_ms,
        ]
    }
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
#[serde(deny_unknown_fields)]
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
    /// Tofu provider-cache layer (acceleration only, never a verdict).
    #[serde(rename = "tofu-providers")]
    TofuProviders,
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
