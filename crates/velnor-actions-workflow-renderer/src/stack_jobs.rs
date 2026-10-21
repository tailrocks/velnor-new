//! Generated jobs of the dbt and Ansible stacks (`dbt-<slug>`, `ansible-<slug>`).
//!
//! One job per project or root: checkout without credentials, the
//! orchestrator's steps (tool provisioning, then parallel checks), and the
//! catalog service containers the stack declares. The job holds
//! `contents: read` only: neither stack has a registry phase, so no step may
//! bind a token, and an `uses:` step is refused outright.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::config::{ANSIBLE_JOB_PREFIX, DBT_JOB_PREFIX};
use velnor_actions_contract::{
    Job, JobTimeout, PermissionLevel, Permissions, ServiceContainer, Step, StepKind, StepRole,
};

use crate::{MiseSetup, RenderError, mise_setup_step, steps};

/// Hosted runner the stack jobs use (the image the tools cache is qualified for).
pub const STACK_RUNNER: &str = "ubuntu-26.04";

/// Which stack a job belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackJobKind {
    /// A dbt-fusion project job.
    Dbt,
    /// An Ansible root job.
    Ansible,
}

impl StackJobKind {
    /// Job id prefix of the stack.
    #[must_use]
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Dbt => DBT_JOB_PREFIX,
            Self::Ansible => ANSIBLE_JOB_PREFIX,
        }
    }
}

/// One resolved project or root job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackJobPolicy {
    /// Stack of the job.
    pub kind: StackJobKind,
    /// Generated job id (`<prefix><slug>`).
    pub id: String,
    /// Display name (`dbt / <project>`, `Ansible / <root>`).
    pub display_name: String,
    /// Job timeout.
    pub timeout: JobTimeout,
    /// Catalog service containers (validated against the catalog).
    pub services: Vec<ServiceContainer>,
    /// Pinned Mise binary the job's tool resolver and installs run under.
    pub setup: MiseSetup,
    /// Steps after the Mise setup, built by the orchestrator in execution order.
    pub steps: Vec<Step>,
}

impl StackJobPolicy {
    fn validate(&self) -> Result<(), RenderError> {
        let invalid = |code: &str| RenderError::InvalidWorkflow(format!("{code}:{}", self.id));
        if !self.id.starts_with(self.kind.prefix()) {
            return Err(invalid("stack_job_id"));
        }
        if self.steps.is_empty() {
            return Err(invalid("stack_job_steps_empty"));
        }
        let mut seen_parallel = false;
        let mut closed = false;
        for step in &self.steps {
            if step.parallel {
                if closed {
                    return Err(invalid("stack_parallel_group_split"));
                }
                seen_parallel = true;
            } else if seen_parallel {
                closed = true;
            }
            if (matches!(step.kind, StepKind::Action { .. }) || step.role.is_some())
                && !is_family_cache_role(step.role)
            {
                return Err(invalid("stack_job_privileged_step"));
            }
        }
        for service in &self.services {
            service
                .validate()
                .map_err(|_| invalid("stack_job_service_not_in_catalog"))?;
        }
        Ok(())
    }
}

/// Build the job for one project or root.
///
/// # Errors
///
/// Returns [`RenderError`] for an invalid policy or checkout.
pub fn build_stack_job(policy: &StackJobPolicy, checkout_uses: &str) -> Result<Job, RenderError> {
    policy.validate()?;
    let mut all = vec![
        steps::checkout_step(checkout_uses)?,
        mise_setup_step(&policy.setup)?,
    ];
    all.extend(policy.steps.iter().cloned());
    Ok(Job {
        services: policy.services.clone(),
        display_name: policy.display_name.clone(),
        runs_on: STACK_RUNNER.to_owned(),
        timeout_minutes: policy.timeout,
        needs: vec![crate::render::PLAN_JOB_ID.to_owned()],
        condition: Some(crate::plan_gate::not_affected_gate(&policy.id)),
        permissions: Some(Permissions {
            contents: PermissionLevel::Read,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            actions: PermissionLevel::None,
            packages: PermissionLevel::None,
        }),
        cache_mode: None,
        environment: None,
        steps: all,
        check_runner: None,
    })
}

/// True for the generated family cache steps (key, restore, size, save); the
/// cache doctor validates their exact shapes.
pub(crate) fn is_family_cache_role(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::FamilyKeys
                | StepRole::FamilyRestore
                | StepRole::FamilySizeCheck
                | StepRole::FamilySave
        )
    )
}

/// Reconstruct the job and reject an altered, missing or split one.
///
/// # Errors
///
/// Returns [`RenderError`] naming the first mismatch.
pub(crate) fn validate_stack_job(
    jobs: &BTreeMap<String, Job>,
    policy: &StackJobPolicy,
    checkout_uses: &str,
) -> Result<Vec<String>, RenderError> {
    let actual = jobs
        .get(&policy.id)
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("stack_job_missing:{}", policy.id)))?;
    let expected = build_stack_job(policy, checkout_uses)?;
    let same = actual.display_name == expected.display_name
        && actual.runs_on == expected.runs_on
        && actual.timeout_minutes == expected.timeout_minutes
        && actual.needs == expected.needs
        && actual.condition == expected.condition
        && actual.permissions == expected.permissions
        && actual.environment == expected.environment
        && actual.services == expected.services
        && actual.steps == expected.steps;
    if !same {
        return Err(RenderError::InvalidWorkflow(format!(
            "stack_job_contract:{}",
            policy.id
        )));
    }
    let ids: BTreeSet<&String> = BTreeSet::from([&policy.id]);
    Ok(ids.into_iter().cloned().collect())
}

#[cfg(test)]
#[path = "stack_jobs_tests.rs"]
mod tests;
