//! Complete qualified Rust inventory capture for authenticated publication.

use velnor_actions_contract::{Stack, StackCandidate};
use velnor_actions_rust::WorkspaceRecord;

use super::PlannedWorkspace;

/// Publish only complete inventories whose every workspace was qualified.
pub(super) fn publishable_inventories(
    candidates: &[StackCandidate],
    outcomes: &[velnor_actions_contract::CandidateOutcome],
    inventories: Vec<(String, WorkspaceRecord)>,
    workspaces: &[PlannedWorkspace],
) -> Vec<(String, WorkspaceRecord)> {
    let complete = candidates
        .iter()
        .zip(outcomes)
        .all(|(candidate, outcome)| candidate.stack_id != Stack::Rust.id() || outcome.metadata_ok);
    let qualified = inventories.iter().all(|(_, record)| {
        workspaces
            .iter()
            .any(|workspace| workspace.record.workspace_root == record.workspace_root)
    });
    if complete && qualified {
        inventories
    } else {
        Vec::new()
    }
}
