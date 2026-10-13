//! Job-level GC policy for jobs that restore or publish MBX objects.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::Job;

/// Return hosted jobs where the MBX action's collector is disabled by default.
///
/// The renderer attaches this policy before lane sharing extracts steps into
/// composites, so all hosted consuming steps inherit the job environment.
pub(crate) fn jobs_with_hosted_mbx_objects(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| {
            velnor_actions_contract::target_for_runner_label(&job.runs_on).is_some()
                && job.steps.iter().any(crate::cache_steps::is_mbx_action)
        })
        .map(|(id, _)| id.clone())
        .collect()
}
