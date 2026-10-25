use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Job, JobTimeout, PermissionLevel, Permissions, Step, StepKind, StepRole,
};

use crate::{RenderError, steps};

pub const BUN_RUNNER: &str = "ubuntu-26.04";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BunMemberPolicy {
    pub name: String,
    pub id: String,
    pub timeout: JobTimeout,
    pub steps: Vec<Step>,
}

impl BunMemberPolicy {
    #[must_use]
    pub fn reads_registry(&self) -> bool {
        reads_registry(&self.steps)
    }

    fn validate(&self) -> Result<(), RenderError> {
        validate_bun_steps(&self.id, &self.steps)
    }
}

pub(crate) fn validate_bun_steps(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    let invalid = |code: &str| RenderError::InvalidWorkflow(format!("{code}:{id}"));
    if !id.starts_with(velnor_actions_contract::config::BUN_JOB_PREFIX) {
        return Err(invalid("bun_job_id"));
    }
    if steps.is_empty() {
        return Err(invalid("bun_job_steps_empty"));
    }
    let mut seen_parallel = false;
    let mut closed = false;
    for step in steps {
        if step.parallel {
            if closed {
                return Err(invalid("bun_parallel_group_split"));
            }
            seen_parallel = true;
        } else if seen_parallel {
            closed = true;
        }
        if matches!(&step.kind, StepKind::Shell { env, .. }
            if env.contains_key(crate::matrix::bun::BUN_LEGS_ENV)
                || env.contains_key(crate::matrix::bun::BUN_MAX_PARALLEL_ENV))
        {
            return Err(invalid("bun_matrix_marker_preset"));
        }
        if matches!(step.kind, StepKind::Action { .. })
            && step.role != Some(StepRole::PrivateTokenMint)
            && !crate::verification_jobs::stack_jobs::is_family_cache_role(step.role)
            && !crate::verification_jobs::bun_artifact_steps::is_bun_artifact_step(step)
        {
            return Err(invalid("bun_job_action_step"));
        }
    }
    validate_private_block(steps, &invalid)?;
    let installs: Vec<usize> = steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(StepRole::PlaywrightInstall))
        .map(|(index, _)| index)
        .collect();
    if installs.len() > 1 {
        return Err(invalid("bun_playwright_install_repeated"));
    }
    if let Some(&at) = installs.first()
        && steps[..at].iter().any(|step| step.parallel)
    {
        return Err(invalid("bun_playwright_install_late"));
    }
    if steps.iter().filter(|step| is_registry_phase(step)).count() > 1 {
        return Err(invalid("bun_registry_phase_repeated"));
    }
    Ok(())
}

fn validate_private_block(
    steps: &[Step],
    invalid: &dyn Fn(&str) -> RenderError,
) -> Result<(), RenderError> {
    let roles: Vec<Option<StepRole>> = steps.iter().map(|step| step.role).collect();
    let at = roles
        .iter()
        .position(|role| *role == Some(StepRole::PrivateTokenMint));
    let private = |role: &Option<StepRole>| {
        matches!(
            role,
            Some(StepRole::PrivateTokenMint | StepRole::PrivateFetch | StepRole::PrivateRevoke)
        )
    };
    let count = roles.iter().filter(|role| private(role)).count();
    let Some(at) = at else {
        return if count == 0 {
            Ok(())
        } else {
            Err(invalid("bun_job_private_block"))
        };
    };
    if count != 3
        || roles.get(at + 1) != Some(&Some(StepRole::PrivateFetch))
        || roles.get(at + 2) != Some(&Some(StepRole::PrivateRevoke))
    {
        return Err(invalid("bun_job_private_block"));
    }
    let only_staging = |role: &Option<StepRole>| *role == Some(StepRole::AcquireVelnor);
    if !roles[..at].iter().all(only_staging) {
        return Err(invalid("bun_job_private_block_late"));
    }
    Ok(())
}

fn is_registry_phase(step: &Step) -> bool {
    step.role == Some(StepRole::BunRegistryInstall)
}

pub(crate) fn reads_registry(steps: &[Step]) -> bool {
    steps.iter().any(is_registry_phase)
}

pub(crate) fn bun_permissions(steps: &[Step]) -> Permissions {
    Permissions {
        contents: PermissionLevel::Read,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::None,
        packages: if reads_registry(steps) {
            PermissionLevel::Read
        } else {
            PermissionLevel::None
        },
    }
}

pub fn build_bun_member_job(
    policy: &BunMemberPolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    policy.validate()?;
    let mut all = vec![steps::checkout_step(checkout_uses)?];
    all.extend(policy.steps.iter().cloned());
    Ok(Job {
        services: Vec::new(),
        display_name: format!("Bun / {}", policy.name),
        runs_on: BUN_RUNNER.to_owned(),
        timeout_minutes: policy.timeout,
        needs: vec![crate::render::PLAN_JOB_ID.to_owned()],
        condition: Some(crate::plan_gate::not_affected_gate(&policy.id)),
        permissions: Some(bun_permissions(&policy.steps)),
        cache_mode: None,
        environment: None,
        steps: all,
        check_runner: None,
    })
}

pub(crate) fn validate_bun_member_job(
    jobs: &BTreeMap<String, Job>,
    policy: &BunMemberPolicy,
    checkout_uses: &str,
) -> Result<Vec<String>, RenderError> {
    let actual = jobs.get(&policy.id).ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("bun_member_job_missing:{}", policy.id))
    })?;
    let expected = build_bun_member_job(policy, checkout_uses)?;
    if !same_bun_job(actual, &expected) {
        return Err(RenderError::InvalidWorkflow(format!(
            "bun_member_job_contract:{}",
            policy.id
        )));
    }
    let ids: BTreeSet<&String> = BTreeSet::from([&policy.id]);
    Ok(ids.into_iter().cloned().collect())
}

pub(crate) fn same_bun_job(actual: &Job, expected: &Job) -> bool {
    actual.display_name == expected.display_name
        && actual.runs_on == expected.runs_on
        && actual.timeout_minutes == expected.timeout_minutes
        && actual.needs == expected.needs
        && actual.condition == expected.condition
        && actual.permissions == expected.permissions
        && actual.environment == expected.environment
        && actual.steps == expected.steps
}
