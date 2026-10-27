// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout, Step, StepKind};

use crate::matrix::bun::{BUN_LEGS_ENV, BUN_MAX_PARALLEL_ENV};
pub use crate::matrix::bun::{BunLeg, BunLegMatrix};
use crate::matrix::suites::SUITE_ALLOW_FAILURE_ENV;

use crate::verification_jobs::bun_member_jobs::{
    BUN_RUNNER, bun_permissions, same_bun_job, validate_bun_steps,
};
use crate::{RenderError, steps};

pub const BUN_SUITE_SEPARATOR: &str = "--";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BunSuitePolicy {
    pub member: String,
    pub suite: String,
    pub id: String,
    pub timeout: JobTimeout,
    pub required: bool,
    pub steps: Vec<Step>,
    pub matrix: Option<BunLegMatrix>,
    pub report: Option<BunReportPolicy>,
    pub assets: Option<String>,
}

pub const BUN_REPORT_SUFFIX: &str = "--report";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BunReportPolicy {
    pub timeout: JobTimeout,
    pub steps: Vec<Step>,
}

impl BunSuitePolicy {
    fn needs(&self) -> Vec<String> {
        let mut needs = vec![crate::render::PLAN_JOB_ID.to_owned()];
        needs.extend(self.assets.iter().cloned());
        needs
    }

    fn validate(&self) -> Result<(), RenderError> {
        let invalid = |code: &str| RenderError::InvalidWorkflow(format!("{code}:{}", self.id));
        if let Some(assets) = &self.assets {
            let member_id = self
                .id
                .split(BUN_SUITE_SEPARATOR)
                .next()
                .unwrap_or_default();
            if *assets != format!("{member_id}{}", super::bun_assets_jobs::BUN_ASSETS_SUFFIX) {
                return Err(invalid("bun_suite_assets_job"));
            }
        }
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
        validate_bun_steps(&self.id, &self.steps)?;
        if let Some(matrix) = &self.matrix {
            matrix.validate()?;
            if self.steps.iter().filter(|step| step.parallel).count() != 1 {
                return Err(invalid("bun_suite_matrix_one_script"));
            }
        }
        if let Some(report) = &self.report {
            if self.matrix.is_none() {
                return Err(invalid("bun_report_without_matrix"));
            }
            validate_bun_steps(&self.report_id(), &report.steps)?;
            if report.steps.iter().any(|step| step.parallel) {
                return Err(invalid("bun_report_parallel_step"));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn report_id(&self) -> String {
        format!("{}{BUN_REPORT_SUFFIX}", self.id)
    }

    #[must_use]
    pub fn job_ids(&self) -> Vec<String> {
        let mut ids = vec![self.id.clone()];
        if self.report.is_some() {
            ids.push(self.report_id());
        }
        ids
    }

    fn marked_steps(&self) -> Vec<Step> {
        let mut steps = self.steps.clone();
        if let Some(matrix) = &self.matrix
            && let Some(step) = steps.iter_mut().find(|step| step.parallel)
            && let StepKind::Shell { env, .. } = &mut step.kind
        {
            env.insert(BUN_LEGS_ENV.to_owned(), matrix.marker());
            if let Some(cap) = matrix.max_parallel {
                env.insert(BUN_MAX_PARALLEL_ENV.to_owned(), cap.to_string());
            }
        }
        steps
    }
}

pub fn build_bun_suite_job(
    policy: &BunSuitePolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    policy.validate()?;
    let mut all = vec![steps::checkout_step(checkout_uses)?];
    all.extend(policy.marked_steps());
    Ok(Job {
        services: Vec::new(),
        display_name: format!("Bun / {} / {}", policy.member, policy.suite),
        runs_on: BUN_RUNNER.to_owned(),
        timeout_minutes: policy.timeout,
        needs: policy.needs(),
        condition: Some(crate::plan_gate::not_affected_gate(&policy.id)),
        permissions: Some(bun_permissions(&policy.steps)),
        cache_mode: None,
        environment: None,
        steps: all,
        check_runner: None,
    })
}

pub fn build_bun_report_job(
    policy: &BunSuitePolicy,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    policy.validate()?;
    let report = policy
        .report
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("bun_report_missing:{}", policy.id)))?;
    let mut all = vec![steps::checkout_step(checkout_uses)?];
    all.extend(report.steps.iter().cloned());
    if let Some(StepKind::Shell { env, .. }) = all
        .iter_mut()
        .rev()
        .find_map(|step| matches!(step.kind, StepKind::Shell { .. }).then_some(&mut step.kind))
    {
        env.insert(SUITE_ALLOW_FAILURE_ENV.to_owned(), "true".to_owned());
    } else {
        return Err(RenderError::InvalidWorkflow(format!(
            "bun_report_without_shell_step:{}",
            policy.id
        )));
    }
    Ok(Job {
        services: Vec::new(),
        display_name: format!("Bun / {} / {} / report", policy.member, policy.suite),
        runs_on: BUN_RUNNER.to_owned(),
        timeout_minutes: report.timeout,
        needs: {
            let mut needs = policy.needs();
            needs.push(policy.id.clone());
            needs
        },
        condition: Some(format!(
            "!cancelled() && needs.{}.result != 'skipped'",
            policy.id
        )),
        permissions: Some(bun_permissions(&report.steps)),
        cache_mode: None,
        environment: None,
        steps: all,
        check_runner: None,
    })
}

pub(crate) fn validate_bun_suite_job(
    jobs: &BTreeMap<String, Job>,
    policy: &BunSuitePolicy,
    checkout_uses: &str,
    plan: Option<&velnor_actions_contract::BunLanePlan>,
) -> Result<Vec<String>, RenderError> {
    let suite_jobs = crate::verification_jobs::bun_lane_validation::project_job(
        &policy.id,
        &build_bun_suite_job(policy, checkout_uses)?,
        plan,
    )?;
    validate_projected_suite_jobs(jobs, &suite_jobs, "bun_suite")?;
    if policy.report.is_some() {
        let report_id = policy.report_id();
        let report_jobs = crate::verification_jobs::bun_lane_validation::project_job(
            &report_id,
            &build_bun_report_job(policy, checkout_uses)?,
            plan,
        )?;
        validate_projected_suite_jobs(jobs, &report_jobs, "bun_report")?;
    }
    Ok(if policy.required {
        crate::verification_jobs::bun_lane_validation::output_ids(&policy.id, plan)?
    } else {
        Vec::new()
    })
}

fn validate_projected_suite_jobs(
    jobs: &BTreeMap<String, Job>,
    expected: &[(String, Job)],
    label: &str,
) -> Result<(), RenderError> {
    for (id, expected_job) in expected {
        let actual = jobs
            .get(id)
            .ok_or_else(|| RenderError::InvalidWorkflow(format!("{label}_job_missing:{id}")))?;
        if !same_bun_job(actual, expected_job) {
            return Err(RenderError::InvalidWorkflow(format!(
                "{label}_job_contract:{id}"
            )));
        }
    }
    Ok(())
}
