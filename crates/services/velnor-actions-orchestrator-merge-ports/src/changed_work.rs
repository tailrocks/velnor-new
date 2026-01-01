//! Changed-work classification shared by planning and baseline coverage.
//!
//! Moved verbatim from `internal::plan_obligation`: the planner assigns
//! dispositions and baseline coverage reuses the same changed
//! definition, so both must judge the identical predicate. Neither side
//! depends on the other; the hub re-exports these from
//! `internal::plan_obligation`.

use std::collections::BTreeSet;

use velnor_actions_contract_planning::ProposedTask;
use velnor_actions_orchestrator_selection::select::group_changed;

/// Changed unit keys for tasks with empty unit IDs.
pub fn changed_keys(universe: &[&ProposedTask], changed: &BTreeSet<String>) -> BTreeSet<String> {
    universe
        .iter()
        .filter(|task| changed.contains(&task.identity.unit_id))
        .map(|task| task.identity.unit_key.clone())
        .collect()
}

/// True when one universe member counts as changed.
pub fn member_changed(
    task: &ProposedTask,
    changed: Option<&BTreeSet<String>>,
    keys: &BTreeSet<String>,
) -> bool {
    changed.is_none_or(|set| group_changed(task, set, keys))
}
