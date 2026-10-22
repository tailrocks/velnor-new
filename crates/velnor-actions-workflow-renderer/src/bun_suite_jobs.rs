//! Bun suite jobs (`bun-<slug>--<suite>`, hard decision 6 amendment).
//!
//! A suite is a declared group of a member's scripts that runs in its own job
//! beside the member job, the way Rust suites and the dependency-seed job sit
//! beside crate jobs. A suite job has the member job's shape: checkout without
//! credentials, the orchestrator's install and script steps, `contents: read`
//! (plus `packages: read` only with the registry phase), and the same step
//! invariants. Only its scripts, browsers and timeout differ.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout, Step};

use crate::verification_jobs::bun_member_jobs::{
    BUN_RUNNER, bun_permissions, same_bun_job, validate_bun_steps,
};
use crate::{RenderError, steps};

/// Separator between a member job id and a suite id; `slug()` never emits it.
pub const BUN_SUITE_SEPARATOR: &str = "--";

/// One resolved suite of one workspace member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BunSuitePolicy {
    /// Member `package.json` name (display).
    pub member: String,
    /// Suite id (`[a-z][a-z0-9-]*`).
    pub suite: String,
    /// Generated job id (`bun-<slug>--<suite>`).
    pub id: String,
    /// Job timeout (each leg's when sharded).
    pub timeout: JobTimeout,
    /// Whether the job joins Required.
    pub required: bool,
    /// Steps after checkout, built by the orchestrator in execution order.
    pub steps: Vec<Step>,
}

impl BunSuitePolicy {
    fn validate(&self) -> Result<(), RenderError> {
        let invalid = |code: &str| RenderError::InvalidWorkflow(format!("{code}:{}", self.id));
        let suffix = format!("{BUN_SUITE_SEPARATOR}{}", self.suite);
        let member_id = self
            .id
            .strip_suffix(&suffix)
            .ok_or_else(|| invalid("bun_suite_job_id"))?;
        if member_id.contains(BUN_SUITE_SEPARATOR)
            || self.suite.is_empty()
            || self.suite.contains(BUN_SUITE_SEPARATOR)
        {
            return Err(invalid("bun_suite_job_id"));
        }
        validate_bun_steps(&self.id, &self.steps)
    }
}

/// Build the job for one suite.
///
/// # Errors
///
/// Returns [`RenderError`] for an invalid policy or timeout.
pub fn build_bun_suite_job(
    policy: &BunSuitePolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    policy.validate()?;
    let mut all = vec![steps::checkout_step(checkout_uses)?];
    all.extend(policy.steps.iter().cloned());
    Ok(Job {
        services: Vec::new(),
        display_name: format!("Bun / {} / {}", policy.member, policy.suite),
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

/// Reconstruct the suite's job and reject an altered or missing one.
///
/// Returns the job ids that join Required: the suite job when it is
/// gating, nothing when `required = false`.
///
/// # Errors
///
/// Returns [`RenderError`] naming the first mismatch.
pub(crate) fn validate_bun_suite_job(
    jobs: &BTreeMap<String, Job>,
    policy: &BunSuitePolicy,
    checkout_uses: &str,
) -> Result<Vec<String>, RenderError> {
    let actual = jobs.get(&policy.id).ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("bun_suite_job_missing:{}", policy.id))
    })?;
    let expected = build_bun_suite_job(policy, checkout_uses)?;
    if !same_bun_job(actual, &expected) {
        return Err(RenderError::InvalidWorkflow(format!(
            "bun_suite_job_contract:{}",
            policy.id
        )));
    }
    Ok(if policy.required {
        vec![policy.id.clone()]
    } else {
        Vec::new()
    })
}
