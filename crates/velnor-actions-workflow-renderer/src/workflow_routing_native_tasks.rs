use std::collections::BTreeMap;

use velnor_actions_contract::config::{BuildTaskRunner, VerificationRunner};
use velnor_actions_contract::{Job, WORKFLOW_TASK_JOB_PREFIX, WorkflowIr};

use crate::{RenderError, verification_jobs::WorkflowTaskPolicy};

use super::{WorkflowRoutingPolicy, invalid};

impl WorkflowRoutingPolicy {
    /// Bind hosted-native routing to task jobs reconstructed from the source IR.
    ///
    /// # Errors
    /// Returns an error when the workflow or any declared task job fails validation.
    pub fn with_verified_workflow_tasks(
        mut self,
        workflow: &WorkflowIr,
        tasks: &[WorkflowTaskPolicy],
        checkout_uses: &str,
    ) -> Result<Self, RenderError> {
        workflow
            .validate()
            .map_err(|error| invalid(format!("typed_workflow_invalid:{error}")))?;
        self.native_hosted_tasks.clear();
        crate::verification_jobs::workflow_task_jobs::validate_workflow_task_jobs(
            &workflow.jobs,
            tasks,
            checkout_uses,
            &self,
        )?;
        self.bind_native_hosted_tasks(workflow, tasks, checkout_uses)?;
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NativeHostedTask {
    pub(super) runner_label: String,
    pub(super) source_job: NativeTaskJobSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NativeTaskJobSnapshot {
    display_name: String,
    runs_on: String,
    check_runner: Option<velnor_actions_contract::config::CheckRunner>,
    timeout_minutes: velnor_actions_contract::JobTimeout,
    needs: Vec<String>,
    condition: Option<String>,
    permissions: Option<velnor_actions_contract::Permissions>,
    cache_mode: Option<velnor_actions_contract::CacheMode>,
    environment: Option<String>,
    services: Vec<velnor_actions_contract::ServiceContainer>,
    steps: Vec<velnor_actions_contract::Step>,
}

impl NativeTaskJobSnapshot {
    pub(super) fn from_job(job: &Job) -> Self {
        Self {
            display_name: job.display_name.clone(),
            runs_on: job.runs_on.clone(),
            check_runner: job.check_runner.clone(),
            timeout_minutes: job.timeout_minutes,
            needs: job.needs.clone(),
            condition: job.condition.clone(),
            permissions: job.permissions.clone(),
            cache_mode: job.cache_mode.clone(),
            environment: job.environment.clone(),
            services: job.services.clone(),
            steps: job.steps.clone(),
        }
    }

    pub(super) fn runs_on(&self) -> &str {
        &self.runs_on
    }

    pub(super) fn condition(&self) -> Option<&str> {
        self.condition.as_deref()
    }
}

pub(super) fn bind_native_hosted_tasks(
    proofs: &mut BTreeMap<String, NativeHostedTask>,
    workflow: &WorkflowIr,
    tasks: &[WorkflowTaskPolicy],
    checkout_uses: &str,
) -> Result<(), RenderError> {
    for task in tasks {
        let Some(runner_label) = native_hosted_runner(task) else {
            continue;
        };
        let job_id = format!("{WORKFLOW_TASK_JOB_PREFIX}{}", task.id());
        let Some(job) = workflow.jobs.get(&job_id) else {
            return Err(invalid(format!("native_hosted_task_missing:{job_id}")));
        };
        if !task.owns_job_id(&job_id)
            || task.runner_label_for_job(&job_id) != Some(runner_label)
            || job.runs_on != runner_label
        {
            return Err(invalid(format!(
                "native_hosted_task_identity_invalid:{job_id}"
            )));
        }
        let expected_job = expected_native_task_job(task, checkout_uses)?;
        if NativeTaskJobSnapshot::from_job(job) != NativeTaskJobSnapshot::from_job(&expected_job) {
            return Err(invalid(format!(
                "native_hosted_task_source_job_invalid:{job_id}"
            )));
        }
        let proof = NativeHostedTask {
            runner_label: runner_label.to_owned(),
            source_job: NativeTaskJobSnapshot::from_job(job),
        };
        if proofs.insert(job_id.clone(), proof).is_some() {
            return Err(invalid(format!("native_hosted_task_duplicate:{job_id}")));
        }
    }
    Ok(())
}

fn expected_native_task_job(
    task: &WorkflowTaskPolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    match task {
        WorkflowTaskPolicy::Build(policy) => {
            crate::verification_jobs::build_build_task_job(policy, checkout_uses)
        }
        WorkflowTaskPolicy::Verification(policy) => {
            crate::verification_jobs::build_verification_task_job(policy, checkout_uses)
        }
        _ => Err(invalid("native_hosted_task_policy_invalid".to_owned())),
    }
}

pub(super) fn native_hosted_runner(task: &WorkflowTaskPolicy) -> Option<&'static str> {
    match task {
        WorkflowTaskPolicy::Build(policy) => match policy.task.runner {
            BuildTaskRunner::Macos26Arm64 | BuildTaskRunner::Xcode27Arm64 => {
                Some(policy.task.runner.runs_on())
            }
        },
        WorkflowTaskPolicy::Verification(policy) => match policy.task.runner {
            VerificationRunner::MacosArm64
            | VerificationRunner::Macos26Arm64
            | VerificationRunner::Xcode27Arm64 => Some(policy.task.runner.runs_on()),
            VerificationRunner::LinuxX64 => None,
        },
        _ => None,
    }
}

pub(super) fn validate_native_task_source(
    policy: &WorkflowRoutingPolicy,
    workflow: &WorkflowIr,
) -> Result<(), RenderError> {
    for (job_id, proof) in &policy.native_hosted_tasks {
        let Some(job) = workflow.jobs.get(job_id) else {
            return Err(invalid(format!(
                "native_hosted_task_source_missing:{job_id}"
            )));
        };
        if NativeTaskJobSnapshot::from_job(job) != proof.source_job {
            return Err(invalid(format!(
                "native_hosted_task_source_changed:{job_id}"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_native_task_inventory(
    workflows: &BTreeMap<String, super::inventory::WorkflowEntry>,
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    if policy.native_hosted_tasks.is_empty() {
        return Ok(());
    }
    let Some(ci) = workflows.get(".github/workflows/ci.yml") else {
        return Err(invalid("native_hosted_task_workflow_missing".to_owned()));
    };
    for job_id in policy.native_hosted_tasks.keys() {
        if !ci.job_ids.contains(job_id) {
            return Err(invalid(format!("native_hosted_task_job_missing:{job_id}")));
        }
    }
    Ok(())
}

pub(super) fn validate_completed_inventory(
    workflows: &BTreeMap<String, super::inventory::WorkflowEntry>,
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    validate_native_task_inventory(workflows, policy)?;
    super::calls::validate_reusable_calls(workflows)
}
