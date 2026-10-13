//! Hosted job policy for MBX objects-cache cleanup.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, RunsOn};

use crate::{cache_steps, yaml::Yaml};

/// Build job-level MBX environment policy before workflow emission.
pub(crate) fn job_env(mbx_job: bool, hosted_linux_mbx_job: bool) -> Option<Yaml> {
    let mut env = Vec::new();
    if mbx_job {
        env.push((
            cache_steps::MBX_GC_AUTO_ENV.to_owned(),
            Yaml::str(cache_steps::MBX_GC_AUTO_VALUE.to_owned()),
        ));
    }
    if hosted_linux_mbx_job {
        env.push(("MBX_SHARE_OUT_DIR".to_owned(), Yaml::str("0")));
    }
    (!env.is_empty()).then_some(Yaml::Map(env))
}

/// Return hosted jobs that use MBX, so action export sees a stable store.
///
/// The renderer attaches this policy before lane sharing extracts steps into
/// composites, so all consuming steps inherit the job environment.
pub(crate) fn hosted_jobs_with_mbx_objects(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| {
            job.steps.iter().any(crate::cache_steps::is_mbx_action) && runs_on_hosted(&job.runs_on)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// Return hosted Linux jobs using MBX; isolated action cleanup removes their store.
pub(crate) fn hosted_linux_jobs_with_mbx_objects(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| {
            job.steps.iter().any(crate::cache_steps::is_mbx_action)
                && runs_on_hosted_linux(&job.runs_on)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// Resolve GitHub-hosted runners from the typed runner selection.
fn runs_on_hosted(runs_on: &str) -> bool {
    matches!(RunsOn::parse(runs_on), Ok(RunsOn::Hosted(_)))
}

/// Resolve GitHub-hosted Linux runners from the typed runner selection.
fn runs_on_hosted_linux(runs_on: &str) -> bool {
    let Ok(runs_on) = RunsOn::parse(runs_on) else {
        return false;
    };
    match runs_on {
        RunsOn::Hosted(label) => label.starts_with("ubuntu-"),
        RunsOn::ScaleSet(_) => false,
    }
}
