//! Typed job-level policy for MBX object-cache consumers.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, ReleaseTarget, RunsOn};

/// Return all typed runner jobs whose steps use MBX objects.
///
/// The renderer inserts `MBX_SHARE_OUT_DIR=0` on these jobs before lane
/// sharing, independent of hosted or Scale Set placement.
pub(crate) fn jobs_with_mbx_objects(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| {
            typed_runner(&job.runs_on) && job.steps.iter().any(crate::cache_steps::is_mbx_action)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// Return hosted Linux jobs whose object cache can use the action backend.
///
/// The cache action's asynchronous collector is disabled only on this
/// hosted lane; Scale Set exports keep MBX's normal collection behavior.
pub(crate) fn jobs_with_hosted_linux_mbx_objects(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| {
            hosted_linux(&job.runs_on) && job.steps.iter().any(crate::cache_steps::is_mbx_action)
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

/// True for a GitHub-hosted Ubuntu label. Scale Set tokens are not Linux hosted.
fn hosted_linux(runs_on: &str) -> bool {
    matches!(
        RunsOn::parse(runs_on),
        Ok(RunsOn::Hosted(label))
            if ReleaseTarget::for_runner_label(&label) == Some(ReleaseTarget::LinuxX86_64)
    )
}
