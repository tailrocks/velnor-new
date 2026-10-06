//! Omit proven covered obligations from the executable plan.

use super::{ObligationDecision, Plan};

/// Drop covered matrix entries and deselect fully-covered packages.
pub(super) fn prune_to_execute(plan: &mut Plan) {
    plan.matrix.include.retain(|entry| {
        plan.obligations
            .iter()
            .any(|ob| ob.task_id == entry.task_id && ob.decision == ObligationDecision::Execute)
    });
    for package in &mut plan.packages {
        package.selected = plan.obligations.iter().any(|ob| {
            ob.decision == ObligationDecision::Execute && package.tasks.contains(&ob.task_id)
        });
    }
}
