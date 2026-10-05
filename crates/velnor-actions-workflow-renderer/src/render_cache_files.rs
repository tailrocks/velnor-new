use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::Job;

use crate::{RenderError, cache_p08, tree::RenderedFile};

pub(crate) fn with_runtime_identity_files(
    mut files: Vec<RenderedFile>,
    jobs: &BTreeMap<String, Job>,
    version: &str,
) -> Result<Vec<RenderedFile>, RenderError> {
    let lanes: BTreeSet<String> = jobs
        .values()
        .filter(|job| {
            job.steps
                .iter()
                .any(|step| step.name == cache_p08::TOOLS_CACHE_IDENTITY_NAME)
        })
        .map(|job| job.runs_on.clone())
        .collect();
    for runs_on in &lanes {
        files.push(cache_p08::runtime_identity_action_file(runs_on, version)?);
    }
    if !lanes.is_empty() {
        files.push(cache_p08::runtime_identity_script_file(version)?);
    }
    Ok(files)
}
