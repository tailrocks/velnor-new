use std::collections::BTreeMap;

use velnor_actions_contract::Job;

use crate::RenderError;

/// Choose one eligible writer for the tools cache.
///
/// Prefer the Plan job, then a job outside a matrix, then the job with the lowest remaining ID.
pub(super) fn choose_tools_cache_writer<'a>(
    owners: &'a [String],
    jobs: &BTreeMap<String, Job>,
) -> Result<&'a str, RenderError> {
    let eligible: Vec<_> = owners
        .iter()
        .filter(|id| jobs.get(*id).is_some_and(|job| !job.is_dependency_seed()))
        .collect();
    eligible
        .iter()
        .copied()
        .find(|id| id.as_str() == crate::render::PLAN_JOB_ID)
        .or_else(|| {
            eligible
                .iter()
                .copied()
                .filter(|id| {
                    jobs.get(*id).is_some_and(|job| {
                        !crate::matrix::suites::has_suite_matrix(job)
                            && !crate::matrix::bun::has_bun_matrix(job)
                    })
                })
                .min()
        })
        .or_else(|| eligible.iter().copied().min())
        .map(String::as_str)
        .ok_or_else(|| RenderError::InvalidWorkflow("tools_cache_writer_missing".to_owned()))
}
