//! Crate-job identity: grouping plus owning-job binding.
//!
//! Runnable obligations group by `(package_id, configuration)`; the
//! contract assigns one stable unbranded job ID per group. Both the
//! renderer ([`build_crate_jobs`](crate::crate_jobs::build_crate_jobs))
//! and the planner resolve through this module, so the plan's
//! per-entry job binding and the rendered jobs can never disagree.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{ProposedTask, assign_crate_job_ids};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

/// Owning job ID for one plan-universe member.
///
/// Package-less workspace tasks belong to the plan job; every other
/// member resolves through the same runnable grouping and ID
/// assignment as the renderer, so the plan's job binding and the
/// rendered jobs can never disagree. `None` only when the member
/// is absent from the assignment inputs (never from the plan path).
pub(crate) fn job_id_for_member(tasks: &[ProposedTask], member: &ProposedTask) -> Option<String> {
    if member.identity.unit_id.is_empty() {
        return Some(PLAN_JOB_ID.to_owned());
    }
    let grouped = group_runnable(tasks);
    let assigned = assign_crate_job_ids(&id_inputs(&grouped));
    assigned
        .get(&(
            member.identity.unit_id.clone(),
            member.configuration.clone(),
        ))
        .cloned()
}

/// Runnable tasks by `(package_id, configuration)` in sorted order.
///
/// Skips tasks without test targets (no command is emitted for them)
/// and package-less workspace tasks (the plan job owns that scope).
pub(crate) fn group_runnable(
    tasks: &[ProposedTask],
) -> BTreeMap<(String, String), Vec<&ProposedTask>> {
    let mut grouped: BTreeMap<(String, String), Vec<&ProposedTask>> = BTreeMap::new();
    for task in tasks {
        if !crate::crate_jobs::is_runnable(task) {
            continue;
        }
        grouped
            .entry((task.identity.unit_id.clone(), task.configuration.clone()))
            .or_default()
            .push(task);
    }
    grouped
}

/// ID-assignment inputs: one `(package_id, package_name, configuration)`
/// triple per crate group, named by its first member.
pub(crate) fn id_inputs(
    grouped: &BTreeMap<(String, String), Vec<&ProposedTask>>,
) -> BTreeSet<(String, String, String)> {
    grouped
        .iter()
        .filter_map(|((package_id, configuration), members)| {
            members.first().map(|first| {
                (
                    package_id.clone(),
                    first.display_name.clone(),
                    configuration.clone(),
                )
            })
        })
        .collect()
}
