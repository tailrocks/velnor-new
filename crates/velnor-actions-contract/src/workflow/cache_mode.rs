//! Literal GitHub cache-service authority; expressions cannot inhabit this type.
use super::{Job, WorkflowIr};
use crate::ContractError;
use serde::{Deserialize, Serialize};

/// GitHub's closed workflow/job cache permission vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheMode {
    /// Restore existing cache entries without publishing.
    Read,
    /// Restore and publish cache entries.
    Write,
    /// Publish entries without restoring them.
    WriteOnly,
    /// Disable both restore and publication.
    None,
}

impl CacheMode {
    /// Exact literal YAML spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::WriteOnly => "write-only",
            Self::None => "none",
        }
    }
}

/// Generated workflows default to read; only closed protected producers write.
pub(crate) fn validate_workflow(ir: &WorkflowIr) -> Result<(), ContractError> {
    if ir.cache_mode != CacheMode::Read {
        return Err(invalid("workflow_requires_read"));
    }
    for job in ir.jobs.values() {
        validate_job(job)?;
    }
    Ok(())
}

/// Role metadata and its canonical condition jointly own publication authority.
/// # Errors
/// Rejects cache publication outside a canonically gated closed producer.
pub fn validate_job(job: &Job) -> Result<(), ContractError> {
    match job.cache_mode {
        None | Some(CacheMode::Read | CacheMode::None) => Ok(()),
        Some(CacheMode::WriteOnly) => Err(invalid("write_only_not_admitted")),
        Some(CacheMode::Write) => {
            let condition = match (&job.tool_producer, &job.source_producer, &job.mbx_producer) {
                (Some(tool), None, None) => tool.selection.condition(tool.descriptor.domain),
                (None, Some(source), None) => source.condition(),
                (None, None, Some(mbx)) => mbx.condition(),
                _ => return Err(invalid("write_requires_pure_producer")),
            };
            if job.condition.as_deref() != Some(condition.as_str()) {
                return Err(invalid("write_requires_protected_producer_condition"));
            }
            Ok(())
        }
    }
}

fn invalid(reason: &str) -> ContractError {
    ContractError::identity("cache_mode", reason)
}

#[cfg(test)]
#[path = "cache_mode_tests.rs"]
mod tests;
