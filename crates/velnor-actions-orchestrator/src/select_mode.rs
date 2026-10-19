//! Event selection modes plus the graph and symlink inputs selection walks.

use std::path::Path;

use velnor_actions_contract::WorkflowEvent;

use crate::discover::Discovery;
use crate::select_affected::Ownership;
use crate::select_symlinks::{ResolvedLink, tracked_links};

/// How one triggering event selects work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionMode {
    /// Only work the change can affect runs; the rest is `not_affected`.
    Affected,
    /// Every obligation runs.
    Full,
}

/// Selection mode per event (hard decision 4).
///
/// Pull requests, fork pull requests included, and local pre-push runs
/// test what they affect. Pushes, merge groups, and dispatched
/// qualification runs test everything: the merge queue and the default
/// branch are the full backstop. Exhaustive on purpose: a new event must
/// choose a mode here.
pub(crate) fn selection_mode(event: WorkflowEvent) -> SelectionMode {
    match event {
        WorkflowEvent::PullRequest | WorkflowEvent::Fork | WorkflowEvent::Local => {
            SelectionMode::Affected
        }
        WorkflowEvent::Push | WorkflowEvent::MergeGroup | WorkflowEvent::Qualification => {
            SelectionMode::Full
        }
    }
}

/// Stable warning token for one event.
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

/// Every workspace's resolved edges, or `None` to broaden.
///
/// A workspace without a complete graph (lockless with registry
/// dependencies, or an unparsable resolve) could reach any package
/// through edges selection cannot see.
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

/// Tracked symlinks resolved at head, or `None` to broaden.
///
/// A link nobody claims never matters: its own change is unowned and
/// broadens anyway. An owned link that escapes the repository, dangles,
/// or loops cannot be bounded, so its owners' reads are unknown.
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
