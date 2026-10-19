//! Typed job-level environment for native MBX jobs.
//!
//! Every job that installs MBX (crate jobs, the Plan helper build, and the
//! Cache seed lanes) receives one identical policy, so a dependency seed and
//! the crate jobs that restore it compile with the same MBX settings.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, RunsOn};

/// Job env every MBX job carries, in key order.
///
/// - `MBX_GC_AUTO=0`: no automatic collection while compilers write.
/// - `MBX_SHARE_OUT_DIR=0`: no shared `OUT_DIR` stabilization.
/// - `MBX_CC=0` / `MBX_BUILD_SCRIPT_EXECUTION=0`: no C or build-script wrappers.
/// - `MBX_TARGET_VIEWS=0`: MBX never moves a fresh CI `target/` into a managed
///   root, so the restored dependency seed stays a plain Cargo target.
pub const MBX_JOB_ENV: [(&str, &str); 5] = [
    ("MBX_BUILD_SCRIPT_EXECUTION", "0"),
    ("MBX_CC", "0"),
    ("MBX_GC_AUTO", "0"),
    ("MBX_SHARE_OUT_DIR", "0"),
    ("MBX_TARGET_VIEWS", "0"),
];

/// Insert [`MBX_JOB_ENV`] into one job env.
pub(crate) fn extend_mbx_job_env(job_env: &mut BTreeMap<String, String>) {
    for (key, value) in MBX_JOB_ENV {
        job_env.insert(key.to_owned(), value.to_owned());
    }
}

/// Return typed runner jobs whose steps use the native MBX action.
///
/// The stable logical store path is physically private to the hosted runner
/// or Scale Set worker namespace. All MBX jobs receive the same policy.
pub(crate) fn jobs_with_mbx(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| {
            typed_runner(&job.runs_on) && job.steps.iter().any(crate::cache_steps::is_mbx_action)
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
