//! Job-level invariants for every native MBX cache owner.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::Job;

/// Return jobs that need MBX GC and shared-OUT_DIR policy in their environment.
///
/// The renderer attaches this policy before lane sharing extracts steps into
/// composites, so all hosted consuming steps inherit the job environment.
pub(crate) fn jobs_with_mbx_objects(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| job.steps.iter().any(crate::cache_steps::is_mbx_action))
        .map(|(id, _)| id.clone())
        .collect()
}
