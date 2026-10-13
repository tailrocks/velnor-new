//! Typed job-level policy for native MBX object-cache consumers.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, RunsOn};

/// Return typed runner jobs whose steps use the native MBX action.
///
/// The stable logical cache path is physically private to the hosted runner
/// or Scale Set worker namespace. All MBX jobs receive the same store policy.
pub(crate) fn jobs_with_mbx_objects(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| {
            typed_runner(&job.runs_on) && job.steps.iter().any(crate::cache_steps::is_mbx_action)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// Return hosted Linux jobs that use the native MBX action.
pub(crate) fn jobs_with_hosted_linux_mbx_objects(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| {
            matches!(RunsOn::parse(&job.runs_on), Ok(RunsOn::Hosted(label)) if label.starts_with("ubuntu-"))
                && job.steps.iter().any(crate::cache_steps::is_mbx_action)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

fn typed_runner(runs_on: &str) -> bool {
    matches!(
        RunsOn::parse(runs_on),
        Ok(RunsOn::Hosted(_) | RunsOn::ScaleSet(_))
    )
}
