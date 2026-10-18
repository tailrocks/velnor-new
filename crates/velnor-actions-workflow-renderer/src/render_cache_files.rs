use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::Job;

use crate::{RenderError, cache_p08, tree::RenderedFile};

#[cfg(test)]
pub(crate) fn with_runtime_identity_files(
    files: Vec<RenderedFile>,
    jobs: &BTreeMap<String, Job>,
    version: &str,
) -> Result<Vec<RenderedFile>, RenderError> {
    append_runtime_files(files, jobs, version, None)
}

pub(crate) fn with_runtime_identity_and_cache_tar_files(
    files: Vec<RenderedFile>,
    jobs: &BTreeMap<String, Job>,
    version: &str,
    staged_binary: &str,
    helper_version: &str,
) -> Result<Vec<RenderedFile>, RenderError> {
    append_runtime_files(files, jobs, version, Some((staged_binary, helper_version)))
}

fn append_runtime_files(
    mut files: Vec<RenderedFile>,
    jobs: &BTreeMap<String, Job>,
    version: &str,
    cache_tar: Option<(&str, &str)>,
) -> Result<Vec<RenderedFile>, RenderError> {
    let lanes: BTreeSet<String> = jobs
        .values()
        .filter(|job| {
            job.steps.iter().any(|step| {
                step.role == Some(velnor_actions_contract::StepRole::ToolsCacheIdentity)
            })
        })
        .map(|job| job.runs_on.clone())
        .collect();
    for runs_on in &lanes {
        files.push(cache_p08::runtime_identity_action_file(runs_on, version)?);
        files.push(cache_p08::runtime_prelude_action_file(runs_on, version)?);
    }
    if !lanes.is_empty() {
        files.push(cache_p08::runtime_identity_script_file(version)?);
        files.push(crate::cache_steps::tools_restore_action_file(version)?);
        files.push(crate::tool_seed::action_file(version)?);
    }
    if jobs.values().any(|job| {
        job.steps
            .iter()
            .any(|step| step.role == Some(velnor_actions_contract::StepRole::CacheTarLaunchers))
    }) {
        let Some((staged_binary, helper_version)) = cache_tar else {
            return Err(RenderError::InvalidWorkflow(
                "cache_tar_composite_context_missing".to_owned(),
            ));
        };
        files.push(crate::cache_tar_action::action_file(
            staged_binary,
            helper_version,
            version,
        )?);
    }
    Ok(files)
}
