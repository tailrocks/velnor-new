//! Conservative local-graph queries over retained metadata.

use std::collections::{BTreeMap, BTreeSet};

use crate::metadata::WorkspaceRecord;
use crate::metadata_edges::LocalEdge;

/// Reverse dependents of `changed` ids using base plus head edges.
///
/// The union of both graphs is considered so added, removed, or renamed
/// edges cannot hide consumers.
#[must_use]
pub fn reverse_closure(
    base: &[LocalEdge],
    head: &[LocalEdge],
    changed: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut reverse: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in base.iter().chain(head.iter()) {
        reverse
            .entry(edge.to.as_str())
            .or_default()
            .push(edge.from.as_str());
    }
    let mut selected = changed.clone();
    let mut stack: Vec<&str> = changed.iter().map(String::as_str).collect();
    while let Some(id) = stack.pop() {
        if let Some(dependents) = reverse.get(id) {
            for dependent in dependents {
                if selected.insert((*dependent).to_owned()) {
                    stack.push(dependent);
                }
            }
        }
    }
    selected
}

/// Deduplicate workspace records by root so each workspace appears once.
#[must_use]
pub fn dedupe_workspaces(mut records: Vec<WorkspaceRecord>) -> Vec<WorkspaceRecord> {
    records.sort_by(|left, right| left.workspace_root.cmp(&right.workspace_root));
    records.dedup_by(|left, right| left.workspace_root == right.workspace_root);
    records
}
