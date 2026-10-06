//! Credential jobs admit only fixed source-owned execution and immutable channels.
use crate::{
    RenderError,
    release_jobs::{ReleaseRole, ReleaseWorkflowSpec},
};

/// Reject any release shape outside the closed source-helper pipeline.
/// # Errors
/// Rejects shell/repository execution, credentials, bindings or ordering drift.
pub fn check_release_jobs(spec: &ReleaseWorkflowSpec) -> Result<(), RenderError> {
    for (id, job) in &spec.jobs {
        spec.bootstrap_tools
            .check_registry(&spec.helper_registry, &job.runs_on)?;
        spec.bootstrap_tools.check_job(id, job)?;
        split::check_job(spec, id, job)?;
        if matches!(
            job.role,
            ReleaseRole::PackageAnonymous | ReleaseRole::PreparationAnonymous
        ) {
            crate::release_checkout_gates::require_exact_checkout(
                id,
                &job.steps,
                &spec.bootstrap.source_sha,
                &spec.repository,
            )?;
        }
    }
    Ok(())
}
#[path = "release_split_gates.rs"]
mod split;
