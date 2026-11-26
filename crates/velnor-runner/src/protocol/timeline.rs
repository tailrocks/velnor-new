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
#[serde(rename_all = "camelCase")]
pub enum TimelineRecordState {
    #[serde(alias = "Pending")]
    Pending,
    #[serde(alias = "InProgress")]
    InProgress,
    #[serde(alias = "Completed")]
    Completed,
}
