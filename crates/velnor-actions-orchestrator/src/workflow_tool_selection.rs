//! Tool domain allocation follows typed task coverage and actual Plan fallback.
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::Job;

use crate::discover::Discovery;

/// Evidence deciding whether one full tool domain needs a runner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Selection {
    pub(super) tasks: BTreeSet<String>,
    pub(super) cargo_fallback: bool,
    pub(super) unconditional: bool,
}

/// Index task identities using the same grouping authority as consumer emission.
pub(super) fn task_index(discovery: &Discovery) -> BTreeMap<String, Vec<String>> {
    let grouped = crate::crate_job_ids::group_runnable(&discovery.proposals);
    let assigned = crate::crate_job_ids::assign_group_ids(&grouped);
    grouped
        .into_iter()
        .filter_map(|(key, tasks)| {
            assigned.get(&key).map(|id| {
                (
                    id.clone(),
                    tasks.iter().map(|task| task.task_id.clone()).collect(),
                )
            })
        })
        .collect()
}

/// Source bootstraps use downstream validation selection, never arbitrary `if:` text.
pub(super) fn for_consumers(
    consumers: &[String],
    jobs: &BTreeMap<String, Job>,
    task_index: &BTreeMap<String, Vec<String>>,
) -> Selection {
    let mut selection = Selection::default();
    for id in consumers {
        if let Some(tasks) = task_index.get(id) {
            selection.tasks.extend(tasks.iter().cloned());
        } else if id == "plan" {
            if jobs
                .get(id)
                .is_some_and(velnor_actions_workflow_renderer::early_plan::has_early_plan)
            {
                selection.cargo_fallback = true;
            } else {
                selection.unconditional = true;
            }
        } else if let Some(producer) = jobs.get(id).and_then(|job| job.source_producer.as_ref()) {
            selection
                .tasks
                .extend(producer.selection.tasks.iter().cloned());
            selection.cargo_fallback |= producer.selection.cargo_fallback;
            selection.unconditional |= producer.selection.unconditional;
        } else {
            selection.unconditional = true;
        }
    }
    selection
}
