//! Generation-only native public attestation authority from compiled SDK owners.
use crate::RenderError;
use velnor_actions_contract::WorkflowIr;

/// Exact immutable workflow approval; never deserialized from repository inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePublishApproval {
    job_id: String,
    workflow: WorkflowIr,
}
impl NativePublishApproval {
    /// Freeze a complete graph produced by a qualified public-repository SDK factory.
    /// This checks structure; the compiled owner must qualify public availability,
    /// exact action pins and the complete helper source/receipt registry separately.
    /// # Errors
    /// Rejects missing roles and malformed source, artifact or credential boundaries.
    pub fn compiled(job_id: &str, workflow: &WorkflowIr) -> Result<Self, RenderError> {
        workflow.validate().map_err(RenderError::Contract)?;
        if workflow
            .jobs
            .get(job_id)
            .is_none_or(|job| job.native_publish.is_none())
        {
            return Err(RenderError::InvalidWorkflow(
                "missing_native_publish_role".into(),
            ));
        }
        Ok(Self {
            job_id: job_id.into(),
            workflow: workflow.clone(),
        })
    }
    /// Bind every step, condition, action, source, artifact and graph dependency.
    #[must_use]
    pub fn admits(&self, job_id: &str, workflow: &WorkflowIr) -> bool {
        self.job_id == job_id && self.workflow == *workflow
    }
}

/// Require exactly one compiled approval for every privileged native role.
/// # Errors
/// Rejects missing, ambiguous or mutated generation authority.
pub(crate) fn validate_approvals(
    workflow: &WorkflowIr,
    approvals: &[NativePublishApproval],
) -> Result<(), RenderError> {
    for (id, job) in &workflow.jobs {
        if job.native_publish.is_some()
            && approvals
                .iter()
                .filter(|approval| approval.admits(id, workflow))
                .count()
                != 1
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "unapproved_native_publish:{id}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "native_publish_approval_tests.rs"]
mod tests;
