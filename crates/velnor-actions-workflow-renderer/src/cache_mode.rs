//! Cache-service authority follows closed producer admission.
use crate::RenderError;
use std::collections::BTreeMap;
use velnor_actions_contract::{CacheMode, Job, WorkflowIr};

pub(crate) fn validate_document(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    writers_admitted: bool,
) -> Result<(), RenderError> {
    if ir.cache_mode != CacheMode::Read {
        return Err(RenderError::InvalidWorkflow(
            "workflow_cache_mode_requires_read".to_owned(),
        ));
    }
    for job in jobs.values() {
        velnor_actions_contract::workflow::cache_mode::validate_job(job)
            .map_err(RenderError::Contract)?;
        if job.cache_mode == Some(CacheMode::Write) && !writers_admitted {
            return Err(RenderError::InvalidWorkflow(
                "cache_writer_requires_strict_admission".to_owned(),
            ));
        }
    }
    Ok(())
}
