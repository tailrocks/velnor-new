use std::collections::BTreeMap;
use std::collections::BTreeSet;

use velnor_actions_contract::Job;

use crate::RenderError;

#[path = "lane_share_job_workflow_selection.rs"]
mod selection;
use super::job_workflow_extract;

pub(super) fn prepare(
    jobs: &BTreeMap<String, Job>,
    job_workflow_ids: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, job_workflow_extract::PreparedJob>, RenderError> {
    if let Some(job_workflow_ids) = job_workflow_ids {
        return selection::prepare(jobs, job_workflow_ids);
    }
    if !jobs.contains_key(job_workflow_extract::PROTOTYPE_LOCAL_JOB_ID) {
        return Ok(BTreeMap::new());
    }
    let Some(job) = jobs.get(job_workflow_extract::PROTOTYPE_JOB_ID) else {
        return Ok(BTreeMap::new());
    };
    let prepared = job_workflow_extract::prepare_job(job_workflow_extract::PROTOTYPE_JOB_ID, job)?;
    Ok(BTreeMap::from([(prepared.source_id.clone(), prepared)]))
}
