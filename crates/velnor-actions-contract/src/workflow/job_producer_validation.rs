//! Mutually exclusive pure producer roles; metadata never grants source admission.
use super::Job;
use crate::ContractError;

pub(super) fn validate(job: &Job) -> Result<(), ContractError> {
    let roles = usize::from(job.source_producer.is_some())
        + usize::from(job.tool_producer.is_some())
        + usize::from(job.mbx_producer.is_some());
    if roles > 1
        || (roles != 0 && (job.native_publish.is_some() || job.native_pages_deploy.is_some()))
    {
        return Err(invalid("conflicting_producer_roles"));
    }
    if let Some(source) = &job.source_producer {
        source.validate()?;
        if source
            .tool_cache
            .as_ref()
            .is_some_and(|tool| tool.runs_on != job.runs_on)
        {
            return Err(invalid("foreign_bootstrap_runner"));
        }
    }
    if let Some(tool) = &job.tool_producer {
        tool.validate()?;
        if job.runs_on != tool.descriptor.runs_on {
            return Err(invalid("invalid_producer_runner"));
        }
    }
    if let Some(mbx) = &job.mbx_producer {
        mbx.validate()?;
        if job.runs_on != mbx.descriptor.runs_on
            || job.needs != mbx.needs()
            || job.condition.as_deref() != Some(mbx.condition().as_str())
            || job.environment.is_some()
        {
            return Err(invalid("invalid_mbx_producer_binding"));
        }
    }
    Ok(())
}

fn invalid(reason: &str) -> ContractError {
    ContractError::identity("job.producer", reason)
}
