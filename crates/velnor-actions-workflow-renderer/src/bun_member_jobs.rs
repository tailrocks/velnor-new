//! Bun workspace member jobs (`bun-<slug>`, hard decision 6).
//!
//! One job per member: checkout without credentials, the orchestrator's
//! install and script steps, nothing else. The job holds `contents: read`;
//! `packages: read` appears only when a Velnor-owned registry-phase step is
//! present, and the credential binding gate already confines
//! `${{ github.token }}` to that step's role.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Job, JobTimeout, PermissionLevel, Permissions, Step, StepKind, StepRole,
};

use crate::{RenderError, steps};

/// Hosted runner the Bun jobs use (the Ubuntu x86-64 image the tools cache is qualified for).
pub const BUN_RUNNER: &str = "ubuntu-26.04";

/// One resolved workspace member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BunMemberPolicy {
    /// Member `package.json` name (display).
    pub name: String,
    /// Generated job id (`bun-<slug>`).
    pub id: String,
    /// Job timeout.
    pub timeout: JobTimeout,
    /// Steps after checkout, built by the orchestrator in execution order.
    pub steps: Vec<Step>,
}

impl BunMemberPolicy {
    /// True when a step runs the registry phase (the only `packages: read` consumer).
    #[must_use]
    pub fn reads_registry(&self) -> bool {
        self.steps.iter().any(is_registry_phase)
    }

    fn validate(&self) -> Result<(), RenderError> {
        let invalid = |code: &str| RenderError::InvalidWorkflow(format!("{code}:{}", self.id));
        if !self
            .id
            .starts_with(velnor_actions_contract::config::BUN_JOB_PREFIX)
        {
            return Err(invalid("bun_job_id"));
        }
        if self.steps.is_empty() {
            return Err(invalid("bun_job_steps_empty"));
        }
        let mut seen_parallel = false;
        let mut closed = false;
        for step in &self.steps {
            if step.parallel {
                if closed {
                    return Err(invalid("bun_parallel_group_split"));
                }
                seen_parallel = true;
            } else if seen_parallel {
                closed = true;
            }
            if matches!(step.kind, StepKind::Action { .. })
                && step.role != Some(StepRole::PrivateTokenMint)
            {
                return Err(invalid("bun_job_action_step"));
            }
        }
        self.validate_private_block(&invalid)?;
        let installs: Vec<usize> = self
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| step.role == Some(StepRole::PlaywrightInstall))
            .map(|(index, _)| index)
            .collect();
        if installs.len() > 1 {
            return Err(invalid("bun_playwright_install_repeated"));
        }
        if let Some(&at) = installs.first()
            && self.steps[..at].iter().any(|step| step.parallel)
        {
            return Err(invalid("bun_playwright_install_late"));
        }
        if self
            .steps
            .iter()
            .filter(|step| is_registry_phase(step))
            .count()
            > 1
        {
            return Err(invalid("bun_registry_phase_repeated"));
        }
        Ok(())
    }
}

impl BunMemberPolicy {
    /// The App-token block is all or nothing: one mint, fetch and revoke in
    /// that order, adjacent, ahead of every step that runs repository code, so
    /// the token is revoked before any install or script (HD-7).
    fn validate_private_block(
        &self,
        invalid: &dyn Fn(&str) -> RenderError,
    ) -> Result<(), RenderError> {
        let roles: Vec<Option<StepRole>> = self.steps.iter().map(|step| step.role).collect();
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
}

/// The registry phase: the one step bound to the npm read token.
fn is_registry_phase(step: &Step) -> bool {
    step.role == Some(StepRole::BunRegistryInstall)
}

/// Build the job for one member.
///
/// # Errors
///
/// Returns [`RenderError`] for an invalid policy or timeout.
pub fn build_bun_member_job(
    policy: &BunMemberPolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    policy.validate()?;
    let mut all = vec![steps::checkout_step(checkout_uses)?];
    all.extend(policy.steps.iter().cloned());
    Ok(Job {
        display_name: format!("Bun / {}", policy.name),
        runs_on: BUN_RUNNER.to_owned(),
        timeout_minutes: policy.timeout,
        needs: vec![crate::render::PLAN_JOB_ID.to_owned()],
        condition: Some(crate::plan_gate::not_affected_gate(&policy.id)),
        permissions: Some(Permissions {
            contents: PermissionLevel::Read,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            actions: PermissionLevel::None,
            packages: if policy.reads_registry() {
                PermissionLevel::Read
            } else {
                PermissionLevel::None
            },
        }),
        cache_mode: None,
        environment: None,
        steps: all,
        check_runner: None,
    })
}

/// Reconstruct the member's job and reject an altered, missing or split one.
///
/// # Errors
///
/// Returns [`RenderError`] naming the first mismatch.
pub(crate) fn validate_bun_member_job(
    jobs: &BTreeMap<String, Job>,
    policy: &BunMemberPolicy,
    checkout_uses: &str,
) -> Result<Vec<String>, RenderError> {
    let actual = jobs.get(&policy.id).ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("bun_member_job_missing:{}", policy.id))
    })?;
    let expected = build_bun_member_job(policy, checkout_uses)?;
    let same = actual.display_name == expected.display_name
        && actual.runs_on == expected.runs_on
        && actual.timeout_minutes == expected.timeout_minutes
        && actual.needs == expected.needs
        && actual.condition == expected.condition
        && actual.permissions == expected.permissions
        && actual.environment == expected.environment
        && actual.steps == expected.steps;
    if !same {
        return Err(RenderError::InvalidWorkflow(format!(
            "bun_member_job_contract:{}",
            policy.id
        )));
    }
    let ids: BTreeSet<&String> = BTreeSet::from([&policy.id]);
    Ok(ids.into_iter().cloned().collect())
}
