//! Remote action pins included in the generated plan report.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, StepKind};

/// Sorted unique pinned remote `uses:` refs across every action step.
#[must_use]
pub fn action_pins(jobs: &BTreeMap<String, Job>) -> Vec<String> {
    let mut pins = BTreeSet::new();
    for job in jobs.values() {
        for step in &job.steps {
            if let StepKind::Action { uses, .. } = &step.kind
                && !uses.starts_with("./.github/actions/")
            {
                pins.insert(uses.clone());
            }
        }
    }
    pins.into_iter().collect()
}
