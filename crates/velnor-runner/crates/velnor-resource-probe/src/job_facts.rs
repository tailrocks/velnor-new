//! Publish one cache admission document to the specified Runner volume.

#[cfg(target_os = "linux")]
mod publish;

use crate::{ProbeError, job_facts_wire::JobFactsPublicationContext};

pub use crate::job_facts_wire::{JobFactsPublicationFile, JobFactsPublicationRecord};

/// Maximum serialized request size for one paired metadata publication.
pub const JOB_FACTS_REQUEST_LIMIT: usize = 176 * 1024;

/// Maximum serialized result size for one paired metadata publication.
pub const JOB_FACTS_RECORD_LIMIT: usize = 24 * 1024;

/// Publish one storage cache admission document at the specified path.
///
/// # Errors
///
/// Returns an error when the document, volume, file, or identity does not meet its specified rules.
#[cfg(target_os = "linux")]
pub fn publish_job_facts(
    context: JobFactsPublicationContext,
) -> Result<JobFactsPublicationRecord, ProbeError> {
    publish::publish(context)
}

/// Reject this operation when the selected guest does not run Linux.
///
/// # Errors
///
/// Returns an error when this guest does not run Linux.
#[cfg(not(target_os = "linux"))]
pub fn publish_job_facts(
    _context: JobFactsPublicationContext,
) -> Result<JobFactsPublicationRecord, ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}
