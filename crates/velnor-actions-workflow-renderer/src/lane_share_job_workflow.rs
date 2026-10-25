use std::collections::BTreeMap;

use velnor_actions_contract::Job;

use crate::RenderError;

use super::job_workflow_extract;

pub(super) fn prepare(
    jobs: &BTreeMap<String, Job>,
) -> Result<BTreeMap<String, job_workflow_extract::PreparedJob>, RenderError> {
    if !jobs.contains_key(job_workflow_extract::PROTOTYPE_LOCAL_JOB_ID) {
        return Ok(BTreeMap::new());
    }
    let Some(job) = jobs.get(job_workflow_extract::PROTOTYPE_JOB_ID) else {
        return Ok(BTreeMap::new());
    };
    let prepared = job_workflow_extract::prepare_job(job_workflow_extract::PROTOTYPE_JOB_ID, job)?;
    Ok(BTreeMap::from([(prepared.source_id.clone(), prepared)]))
}
