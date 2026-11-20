// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(target_os = "linux")]
mod publish;

use crate::{ProbeError, job_facts_wire::JobFactsPublicationContext};

pub use crate::job_facts_wire::{JobFactsPublicationFile, JobFactsPublicationRecord};

pub const JOB_FACTS_REQUEST_LIMIT: usize = 176 * 1024;

pub const JOB_FACTS_RECORD_LIMIT: usize = 24 * 1024;

#[cfg(target_os = "linux")]
pub fn publish_job_facts(
    context: JobFactsPublicationContext,
) -> Result<JobFactsPublicationRecord, ProbeError> {
    publish::publish(context)
}

#[cfg(not(target_os = "linux"))]
pub fn publish_job_facts(
    _context: JobFactsPublicationContext,
) -> Result<JobFactsPublicationRecord, ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}
