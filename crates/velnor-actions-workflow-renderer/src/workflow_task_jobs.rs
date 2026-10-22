//! Variant-dispatched validation and Required fan-in for the one task graph.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, WORKFLOW_TASK_JOB_PREFIX};

use super::{
    build_task_jobs::BuildTaskPolicy, bun_member_jobs::BunMemberPolicy,
    bun_suite_jobs::BunSuitePolicy, image_build_jobs::ImageBuildPolicy,
    native_image_jobs::NativeImageTaskPolicy, stack_jobs::StackJobPolicy,
};
use crate::{RenderError, VerificationTaskPolicy};

#[cfg(test)]
#[path = "workflow_task_jobs_tests.rs"]
mod tests;

/// One source-validated renderer policy for a declared workflow task.
#[derive(Debug, Clone)]
pub enum WorkflowTaskPolicy {
    /// Compile-free verification task, eligible for schema-2 Linux pairs.
    Verification(VerificationTaskPolicy),
    /// Native build task, hosted-only with explicit resource limits.
    Build(BuildTaskPolicy),
    /// Native-platform image validation task, hosted-only on matching hardware.
    NativeImage(NativeImageTaskPolicy),
    /// Container image build jobs, one per native platform, hosted-only.
    Image(ImageBuildPolicy),
    /// One Bun workspace member job (hard decision 6), hosted-only.
    Bun(BunMemberPolicy),
    /// One declared Bun suite job beside its member job, hosted-only.
    BunSuite(BunSuitePolicy),
    /// One dbt project or Ansible root job, hosted-only.
    Stack(StackJobPolicy),
}

impl WorkflowTaskPolicy {
    /// Shared task ID, used to preserve one deterministic namespace.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Verification(policy) => &policy.task.id,
            Self::Build(policy) => &policy.task.id,
            Self::NativeImage(policy) => &policy.task.id,
            Self::Image(policy) => &policy.image.id,
            Self::Bun(policy) => &policy.id,
            Self::BunSuite(policy) => &policy.id,
            Self::Stack(policy) => &policy.id,
        }
    }

    /// Exact runner label the task variant selects for one of its jobs.
    #[must_use]
    pub fn runner_label_for_job(&self, job_id: &str) -> Option<&str> {
        match self {
            Self::Verification(policy) => Some(&policy.runner_label),
            Self::Build(policy) => Some(&policy.runner_label),
            Self::NativeImage(policy) => Some(&policy.runner_label),
            Self::Image(policy) => policy
                .platform_for_job(job_id)
                .map(velnor_actions_contract::ImagePlatform::runs_on),
            Self::Bun(policy) => {
                (policy.id == job_id).then_some(super::bun_member_jobs::BUN_RUNNER)
            }
            Self::BunSuite(policy) => policy
                .job_ids()
                .iter()
                .any(|id| id == job_id)
                .then_some(super::bun_member_jobs::BUN_RUNNER),
            Self::Stack(policy) => (policy.id == job_id).then_some(super::stack_jobs::STACK_RUNNER),
        }
    }

    /// Whether the task's job runs pinned tools and needs the shared Mise prelude.
    #[must_use]
    pub fn uses_pinned_tools(&self) -> bool {
        match self {
            Self::Image(policy) => policy.uses_pinned_tools(),
            Self::Bun(_) | Self::BunSuite(_) | Self::Stack(_) => true,
            _ => false,
        }
    }

    /// Whether a job belongs to this task's permitted placement set.
    #[must_use]
    pub fn owns_job_id(&self, job_id: &str) -> bool {
        match self {
            Self::Verification(policy) => policy.owns_job_id(job_id),
            Self::Build(policy) => policy.job_id() == job_id,
            Self::NativeImage(policy) => policy.job_id() == job_id,
            Self::Image(policy) => policy.platform_for_job(job_id).is_some(),
            Self::Bun(policy) => policy.id == job_id,
            Self::BunSuite(policy) => policy.job_ids().iter().any(|id| id == job_id),
            Self::Stack(policy) => policy.id == job_id,
        }
    }
}

/// Reconstruct each task variant and reject every undeclared task-prefixed job.
/// # Errors
pub(crate) fn validate_workflow_task_jobs(
    jobs: &BTreeMap<String, Job>,
    policies: &[WorkflowTaskPolicy],
    checkout_uses: &str,
    routing: &crate::workflow_routing::WorkflowRoutingPolicy,
) -> Result<Vec<String>, RenderError> {
    let mut previous = None;
    let mut seen = BTreeSet::new();
    let mut job_ids = Vec::new();
    for policy in policies {
        let id = policy.id();
        if previous.is_some_and(|previous_id: &str| previous_id >= id) || !seen.insert(id) {
            return Err(RenderError::InvalidWorkflow(
                "workflow_tasks_not_sorted_unique".to_owned(),
            ));
        }
        match policy {
            WorkflowTaskPolicy::Verification(task) => {
                job_ids.extend(crate::verification_jobs::validate_verification_jobs(
                    jobs,
                    std::slice::from_ref(task),
                    checkout_uses,
                )?);
            }
            WorkflowTaskPolicy::Build(task) => job_ids.extend(
                crate::verification_jobs::build_task_jobs::validate_build_task_jobs(
                    jobs,
                    std::slice::from_ref(task),
                    checkout_uses,
                )?,
            ),
            WorkflowTaskPolicy::Image(task) => job_ids.extend(
                crate::verification_jobs::image_build_jobs::validate_image_build_jobs(
                    jobs,
                    task,
                    checkout_uses,
                    routing,
                )?,
            ),
            WorkflowTaskPolicy::Bun(task) => job_ids.extend(
                crate::verification_jobs::bun_member_jobs::validate_bun_member_job(
                    jobs,
                    task,
                    checkout_uses,
                )?,
            ),
            WorkflowTaskPolicy::BunSuite(task) => job_ids.extend(
                crate::verification_jobs::bun_suite_jobs::validate_bun_suite_job(
                    jobs,
                    task,
                    checkout_uses,
                )?,
            ),
            WorkflowTaskPolicy::Stack(task) => {
                let ids = crate::verification_jobs::stack_jobs::validate_stack_job(
                    jobs,
                    task,
                    checkout_uses,
                )?;
                job_ids.extend(ids);
            }
            WorkflowTaskPolicy::NativeImage(task) => job_ids.extend(
                crate::verification_jobs::native_image_jobs::validate_native_image_jobs(
                    jobs,
                    std::slice::from_ref(task),
                    checkout_uses,
                )?,
            ),
        }
        previous = Some(id);
    }
    job_ids.sort();
    let declared = job_ids.iter().collect::<BTreeSet<_>>();
    if jobs
        .keys()
        .any(|id| id.starts_with(WORKFLOW_TASK_JOB_PREFIX) && !declared.contains(&id))
    {
        return Err(RenderError::InvalidWorkflow(
            "undeclared_workflow_task_job".to_owned(),
        ));
    }
    Ok(job_ids)
}

/// Add every emitted job from the shared task graph to Required's success fan-in.
/// # Errors
pub(crate) fn extend_required_needs(
    jobs: &mut BTreeMap<String, Job>,
    task_job_ids: &[String],
) -> Result<(), RenderError> {
    if task_job_ids.is_empty() {
        return Ok(());
    }
    let required = jobs.get_mut(crate::render::FINAL_JOB_ID).ok_or_else(|| {
        RenderError::InvalidWorkflow("workflow_tasks_require_required_job".to_owned())
    })?;
    for id in task_job_ids {
        if !required.needs.contains(id) {
            required.needs.push(id.clone());
        }
    }
    required.needs.sort();
    required.needs.dedup();
    Ok(())
}
