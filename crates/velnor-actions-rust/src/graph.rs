//! Conservative local-graph queries over retained metadata.

use crate::metadata::WorkspaceRecord;

/// Deduplicate workspace records by root so each workspace appears once.
#[must_use]
pub fn dedupe_workspaces(mut records: Vec<WorkspaceRecord>) -> Vec<WorkspaceRecord> {
    records.sort_by(|left, right| left.workspace_root.cmp(&right.workspace_root));
    records.dedup_by(|left, right| left.workspace_root == right.workspace_root);
    records
}
