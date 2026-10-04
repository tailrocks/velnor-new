//! Job-level GC policy for jobs that produce or export MBX objects.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, RunsOn};

/// Return jobs that use MBX, so automatic collection stays off through export.
///
/// The renderer attaches this policy before lane sharing extracts steps into
/// composites, so all consuming steps inherit the job environment.
pub(crate) fn jobs_with_mbx_objects(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| job.steps.iter().any(crate::cache_steps::is_mbx_action))
        .map(|(id, _)| id.clone())
        .collect()
}

/// Return Linux jobs that use MBX; only these can create readonly OUT_DIRs
/// that the action cleanup or Scale Set exporter must remove.
pub(crate) fn linux_jobs_with_mbx_objects(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| {
            job.steps.iter().any(crate::cache_steps::is_mbx_action)
                && runs_on_linux(&job.runs_on)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// Resolve Linux from the typed runner label. Scale Set names follow the
/// product convention of naming Linux workers with `ubuntu-` or `linux-`.
fn runs_on_linux(runs_on: &str) -> bool {
    let Ok(runs_on) = RunsOn::parse(runs_on) else {
        return false;
    };
    match runs_on {
        RunsOn::Hosted(label) => label.starts_with("ubuntu-"),
        RunsOn::ScaleSet(selector) => selector
            .labels()
            .iter()
            .any(|label| label.starts_with("ubuntu-") || label.starts_with("linux-")),
    }
}
