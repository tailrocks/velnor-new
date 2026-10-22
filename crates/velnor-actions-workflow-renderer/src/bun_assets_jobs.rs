//! The one job that fetches a member's private release assets for its suites
//! (`bun-<slug>--assets`, HD-7).
//!
//! Copying the App-token block into every suite job (and every matrix leg)
//! would put the secret in N jobs. Instead this job alone runs the staged
//! helper, mint, fetch and revoke, indexes the verified `.velnor/deps` tree
//! file by file, and uploads it as a one-day artifact of this run. Suite jobs
//! download it, check every file against the index and every archive digest
//! the lock pins against `verified.json`, and hold no credential.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout, Step, StepRole};

use crate::verification_jobs::bun_member_jobs::{
    BUN_RUNNER, bun_permissions, same_bun_job, validate_bun_steps,
};
use crate::{RenderError, steps};

/// Job id suffix of a member's assets job.
pub const BUN_ASSETS_SUFFIX: &str = "--assets";

/// The assets job of one member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BunAssetsPolicy {
    /// Member `package.json` name (display).
    pub member: String,
    /// Generated job id (`bun-<slug>--assets`).
    pub id: String,
    /// Job timeout (the member job's).
    pub timeout: JobTimeout,
    /// Steps after checkout: helper, mint, fetch, revoke, index, upload.
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

/// Build the assets job.
///
/// # Errors
///
/// Returns [`RenderError`] for an invalid policy.
pub fn build_bun_assets_job(
    policy: &BunAssetsPolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    policy.validate()?;
    let mut all = vec![steps::checkout_step(checkout_uses)?];
    all.extend(policy.steps.iter().cloned());
    Ok(Job {
        services: Vec::new(),
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

/// Reconstruct the assets job. It never joins Required itself: its suites
/// need it, so a failed fetch skips them and Required fails closed
/// (`planned_job_skipped`).
///
/// # Errors
///
/// Returns [`RenderError`] naming the first mismatch.
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
