// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::metadata::WorkspaceRecord;

#[must_use]
pub fn dedupe_workspaces(mut records: Vec<WorkspaceRecord>) -> Vec<WorkspaceRecord> {
    records.sort_by(|left, right| left.workspace_root.cmp(&right.workspace_root));
    records.dedup_by(|left, right| left.workspace_root == right.workspace_root);
    records
}
