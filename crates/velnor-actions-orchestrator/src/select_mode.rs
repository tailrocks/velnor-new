use std::path::Path;

use velnor_actions_contract::{WorkflowEvent, selects_affected_work};

use crate::discover::Discovery;
use crate::select_affected::Ownership;
use crate::select_symlinks::{ResolvedLink, tracked_links};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionMode {
    Affected,
    Full,
}

pub(crate) fn selection_mode(event: WorkflowEvent) -> SelectionMode {
    if selects_affected_work(event) {
        SelectionMode::Affected
    } else {
        SelectionMode::Full
    }
}

pub(super) fn event_token(event: WorkflowEvent) -> &'static str {
    match event {
        WorkflowEvent::PullRequest => "pull_request",
        WorkflowEvent::Push => "push",
        WorkflowEvent::MergeGroup => "merge_group",
        WorkflowEvent::Local => "local",
        WorkflowEvent::Fork => "fork",
        WorkflowEvent::Qualification => "qualification",
    }
}

pub(super) fn resolved_edges(
    discovery: &Discovery,
    warnings: &mut Vec<String>,
) -> Option<Vec<(String, String)>> {
    let mut resolved = Vec::new();
    for resolve in &discovery.affected.resolves {
        match &resolve.graph {
            Ok(graph) => resolved.extend(graph.edges.iter().cloned()),
            Err(problem) => {
                let root = if resolve.workspace_root.is_empty() {
                    "."
                } else {
                    resolve.workspace_root.as_str()
                };
                warnings.push(format!(
                    "comparison_unavailable:resolve_graph:{root}:{problem}:all_changed"
                ));
                return None;
            }
        }
    }
    Some(resolved)
}

pub(super) fn resolved_links(
    root: &Path,
    ownership: &Ownership<'_>,
    warnings: &mut Vec<String>,
) -> Option<Vec<ResolvedLink>> {
    let outcomes = match tracked_links(root) {
        Ok(outcomes) => outcomes,
        Err(problem) => {
            warnings.push(format!(
                "comparison_unavailable:symlinks:{problem}:all_changed"
            ));
            return None;
        }
    };
    let mut links = Vec::with_capacity(outcomes.len());
    for outcome in outcomes {
        match outcome {
            Ok(link) => links.push(link),
            Err((path, problem)) => {
                if !ownership.claim(&path).is_empty() {
                    warnings.push(format!("symlink_unbounded:{path}:{problem}:all_changed"));
                    return None;
                }
            }
        }
    }
    Some(links)
}
