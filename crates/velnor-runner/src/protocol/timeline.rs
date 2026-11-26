// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskResult {
    #[serde(alias = "Succeeded")]
    Succeeded,
    #[serde(alias = "SucceededWithIssues")]
    SucceededWithIssues,
    #[serde(alias = "Failed")]
    Failed,
    #[serde(alias = "Canceled")]
    Canceled,
    #[serde(alias = "Skipped")]
    Skipped,
    #[serde(alias = "Abandoned")]
    Abandoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimelineRecordType {
    #[serde(alias = "job")]
    Job,
    #[serde(alias = "task")]
    Task,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TimelineRecordState {
    #[serde(alias = "Pending")]
    Pending,
    #[serde(alias = "InProgress")]
    InProgress,
    #[serde(alias = "Completed")]
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRecord {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(rename = "type")]
    pub record_type: TimelineRecordType,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_operation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent_complete: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<TimelineRecordState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<TaskResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(default)]
    pub error_count: i32,
    #[serde(default)]
    pub warning_count: i32,
    #[serde(default)]
    pub notice_count: i32,
}

impl TimelineRecord {
    pub fn pending(
        id: impl Into<String>,
        record_type: TimelineRecordType,
        name: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            parent_id: None,
            record_type,
            name: name.into(),
            start_time: None,
            finish_time: None,
            current_operation: None,
            percent_complete: Some(0),
            state: Some(TimelineRecordState::Pending),
            result: None,
            worker_name: None,
            order: None,
            ref_name: None,
            error_count: 0,
            warning_count: 0,
            notice_count: 0,
        }
    }

    #[must_use]
    pub fn with_parent(mut self, parent_id: impl Into<String>, order: i32) -> Self {
        self.parent_id = Some(parent_id.into());
        self.order = Some(order);
        self
    }

    #[must_use]
    pub fn with_worker(mut self, worker_name: impl Into<String>, ref_name: Option<String>) -> Self {
        self.worker_name = Some(worker_name.into());
        self.ref_name = ref_name;
        self
    }

    #[must_use]
    pub const fn with_issue_counts(mut self, error: i32, warning: i32, notice: i32) -> Self {
        self.error_count = error;
        self.warning_count = warning;
        self.notice_count = notice;
        self
    }

    #[must_use]
    pub fn in_progress(mut self, start_time: impl Into<String>) -> Self {
        self.start_time = Some(start_time.into());
        self.state = Some(TimelineRecordState::InProgress);
        self
    }

    #[must_use]
    pub fn completed(mut self, finish_time: impl Into<String>, result: TaskResult) -> Self {
        self.finish_time = Some(finish_time.into());
        self.percent_complete = Some(100);
        self.state = Some(TimelineRecordState::Completed);
        self.result = Some(result);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRecordFeedLines {
    pub step_id: String,
    pub value: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<i64>,
}

impl TimelineRecordFeedLines {
    pub fn new(step_id: impl Into<String>, lines: Vec<String>, start_line: Option<i64>) -> Self {
        Self {
            step_id: step_id.into(),
            value: lines,
            start_line,
        }
    }
}
