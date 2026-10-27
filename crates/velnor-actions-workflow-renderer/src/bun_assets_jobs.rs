// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout, Step, StepRole};

use crate::verification_jobs::bun_member_jobs::{
    BUN_RUNNER, bun_permissions, same_bun_job, validate_bun_steps,
};
use crate::{RenderError, steps};

pub const BUN_ASSETS_SUFFIX: &str = "--assets";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BunAssetsPolicy {
    pub member: String,
    pub id: String,
    pub timeout: JobTimeout,
    pub steps: Vec<Step>,
}

impl BunAssetsPolicy {
    fn validate(&self) -> Result<(), RenderError> {
        let invalid = |code: &str| RenderError::InvalidWorkflow(format!("{code}:{}", self.id));
        if !self.id.ends_with(BUN_ASSETS_SUFFIX) {
            return Err(invalid("bun_assets_job_id"));
        }
        validate_bun_steps(&self.id, &self.steps)?;
        if !self
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::PrivateFetch))
        {
            return Err(invalid("bun_assets_without_fetch"));
        }
        if self.steps.iter().any(|step| step.parallel) {
            return Err(invalid("bun_assets_parallel_step"));
        }
        Ok(())
    }
}

pub fn build_bun_assets_job(
    policy: &BunAssetsPolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    policy.validate()?;
    let mut all = vec![steps::checkout_step(checkout_uses)?];
    all.extend(policy.steps.iter().cloned());
    Ok(Job {
        services: Vec::new(),
        outputs: BTreeMap::new(),
        display_name: format!("Bun / {} / assets", policy.member),
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

pub(crate) fn validate_bun_assets_job(
    jobs: &BTreeMap<String, Job>,
    policy: &BunAssetsPolicy,
    checkout_uses: &str,
) -> Result<Vec<String>, RenderError> {
    let actual = jobs.get(&policy.id).ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("bun_assets_job_missing:{}", policy.id))
    })?;
    if !same_bun_job(actual, &build_bun_assets_job(policy, checkout_uses)?) {
        return Err(RenderError::InvalidWorkflow(format!(
            "bun_assets_job_contract:{}",
            policy.id
        )));
    }
    Ok(Vec::new())
}
