//! Narrow changed-hint refinement; authenticated complete closures still decide.

use velnor_actions_contract::{ProposedTask, Stack};

use crate::merge::required_evidence::BaselineTaskEntry;
use crate::select::ChangedSelection;

/// Only qualified Rust package uncertainty may reach the live proof gates.
pub(super) fn permits(
    task: &ProposedTask,
    changed: Option<&ChangedSelection>,
    universe: &[&ProposedTask],
    entry: &BaselineTaskEntry,
) -> bool {
    let Some(changed) = changed else {
        return false;
    };
    if Stack::from_id(&task.stack_id) != Some(Stack::Rust) || entry.proof.is_none() {
        return false;
    }
    if !task.identity.unit_id.is_empty() {
        return changed.proof_refinable.contains(&task.identity.unit_id);
    }
    // Empty-ID groups inherit their manifest owners; every owner must qualify.
    let owners: Vec<_> = universe
        .iter()
        .filter(|owner| {
            Stack::from_id(&owner.stack_id) == Some(Stack::Rust)
                && owner.identity.unit_key == task.identity.unit_key
                && !owner.identity.unit_id.is_empty()
        })
        .collect();
    !owners.is_empty()
        && owners
            .iter()
            .all(|owner| changed.proof_refinable.contains(&owner.identity.unit_id))
}

#[cfg(test)]
#[path = "cover_refinement_tests.rs"]
mod tests;
