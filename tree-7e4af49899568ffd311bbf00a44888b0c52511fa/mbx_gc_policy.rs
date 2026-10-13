//! Job-level env for hosted Linux jobs that restore MBX objects.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, RunsOn};

/// Return hosted Linux jobs whose steps restore MBX objects.
///
/// The renderer inserts `MBX_GC_AUTO=0` and `MBX_SHARE_OUT_DIR=0` on these
/// jobs before lane sharing. Scale Set jobs stay out of the set, so they
/// keep their existing collection and `OUT_DIR` behavior.
pub(crate) fn jobs_with_hosted_linux_mbx_objects(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| {
            hosted_linux(&job.runs_on) && job.steps.iter().any(crate::cache_steps::is_mbx_action)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// True for a GitHub-hosted Ubuntu label. Scale Set tokens are not Linux hosted.
fn hosted_linux(runs_on: &str) -> bool {
    matches!(
        RunsOn::parse(runs_on),
        Ok(RunsOn::Hosted(label)) if label.starts_with("ubuntu-")
    )
}
