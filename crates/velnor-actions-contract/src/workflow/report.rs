// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::workflow::plan::WorkflowEvent;
use crate::workflow::platform::PlatformBinding;
use crate::workflow::trust::Trust;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReport {
    pub schema: u32,

    pub task_report_id: String,

    pub run_key: String,

    pub event: WorkflowEvent,

    pub trust: Trust,

    pub matrix_id: String,

    pub matrix_key: String,

    pub task_id: String,

    pub task_digest: String,

    pub status: TaskStatus,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_selected_reason: Option<NotSelectedReason>,

    pub cache: CacheOutcome,

    pub platform_binding: PlatformBinding,

    pub exit_code: i32,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,

    #[serde(default)]
    pub outputs: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<TaskTiming>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskTiming {
    pub queue_ms: u64,

    pub runner_ms: u64,

    pub task_ms: u64,

    pub cache_ms: u64,

    pub prep_ms: u64,

    pub download_ms: u64,

    pub compiler_ms: u64,

    pub mbx_ms: u64,

    pub test_ms: u64,

    pub lock_wait_ms: u64,
}
impl TaskTiming {
    #[must_use]
    pub fn accounted_total(&self) -> u64 {
        self.slots()
            .iter()
            .fold(0, |sum, slot| sum.saturating_add(*slot))
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Reused,

    Executed,

    EmptyPartition,

    NotSelected,

    Failed,

    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotSelectedReason {
    UpstreamFailed,

    NotInPlan,

    Unsupported,

    CancelledByPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheOutcome {
    pub layer: CacheLayer,

    pub key: String,

    pub result: CacheResult,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub miss_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheLayer {
    Sources,

    Mbx,

    Task,

    #[serde(rename = "tofu-providers")]
    TofuProviders,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheResult {
    Hit,

    Miss,

    NotAttempted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixReport {
    pub schema: u32,

    pub report_id: String,

    pub run_key: String,

    pub matrix_id: String,

    pub matrix_key: String,

    pub status: MatrixStatus,

    pub expected_task_ids: Vec<String>,

    pub task_report_ids: Vec<String>,

    pub tasks: Vec<MatrixTaskEntry>,

    pub selected: u32,

    pub reused: u32,

    pub executed: u32,

    pub empty_partition: u32,

    pub not_selected: u32,

    pub failed: u32,

    pub cancelled: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixTaskEntry {
    pub task_report_id: String,

    pub task_id: String,

    pub status: TaskStatus,

    pub exit_code: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatrixStatus {
    Passed,

    Failed,

    Cancelled,

    NotRun,
}
