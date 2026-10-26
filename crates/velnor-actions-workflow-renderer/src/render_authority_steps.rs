// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, StepRole, WorkflowPolicy};

use crate::render::{FINAL_JOB_ID, PLAN_JOB_ID};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalRuntimeInputs {
    plan_consumer_env: BTreeMap<String, String>,
    report_helper_version: String,
    staged_binary: String,
    request_dir: String,
    checkout_uses: String,
    preseed: bool,
}

impl CanonicalRuntimeInputs {
    pub fn new(
        plan_consumer_env: BTreeMap<String, String>,
        report_helper_version: String,
        staged_binary: String,
        request_dir: String,
        checkout_uses: String,
        preseed: bool,
    ) -> Result<Self, crate::RenderError> {
        validate_plan_consumer_env(&plan_consumer_env)?;
        Ok(Self {
            plan_consumer_env,
            report_helper_version,
            staged_binary,
            request_dir,
            checkout_uses,
            preseed,
        })
    }
    #[must_use]
    pub fn plan_consumer_env(&self) -> &BTreeMap<String, String> {
        &self.plan_consumer_env
    }

    #[must_use]
    pub fn report_helper_version(&self) -> &str {
        &self.report_helper_version
    }

    #[must_use]
    pub fn staged_binary(&self) -> &str {
        &self.staged_binary
    }

    #[must_use]
    pub fn request_dir(&self) -> &str {
        &self.request_dir
    }

    #[must_use]
    pub fn checkout_uses(&self) -> &str {
        &self.checkout_uses
    }

    #[must_use]
    pub fn preseed(&self) -> bool {
        self.preseed
    }
}

#[derive(Debug, Clone)]
pub struct CanonicalAuthoritySteps {
    authority_jobs: Vec<CanonicalAuthorityJobs>,
    mise_setup: Option<Step>,
    required_shells: Vec<Step>,
    producer_plan_shells: Option<Vec<Step>>,
    runtime: CanonicalRuntimeInputs,
}

#[derive(Debug, Clone)]
struct CanonicalAuthorityJobs {
    required: Option<Vec<u8>>,
    producer_plan: Option<Vec<u8>>,
}

impl CanonicalAuthoritySteps {
    pub fn from_native_jobs_with_mise_setup(
        jobs: &BTreeMap<String, Job>,
        policy: WorkflowPolicy,
        setup: &crate::setup::MiseSetup,
        expanded_authority_jobs: &[(Job, Option<Job>)],
        runtime: CanonicalRuntimeInputs,
    ) -> Result<Self, crate::RenderError> {
        if policy == WorkflowPolicy::VelnorRepositoryV1
            && expanded_authority_jobs
                .iter()
                .any(|(_, producer_plan)| producer_plan.is_none())
        {
            return Err(crate::RenderError::InvalidWorkflow(
                "trusted_plan_candidate_execution".to_owned(),
            ));
        }
        let setup = crate::setup::mise_setup_step(setup)?;
        let variants = expanded_authority_jobs
            .iter()
            .cloned()
            .map(|(required, plan)| (Some(required), plan))
            .collect::<Vec<_>>();
        Self::from_native_jobs_inner(jobs, policy, Some(setup), &variants, runtime)
    }

    fn from_native_jobs_inner(
        jobs: &BTreeMap<String, Job>,
        policy: WorkflowPolicy,
        mise_setup: Option<Step>,
        expanded_authority_jobs: &[(Option<Job>, Option<Job>)],
        runtime: CanonicalRuntimeInputs,
    ) -> Result<Self, crate::RenderError> {
        let mut authority_jobs = vec![CanonicalAuthorityJobs {
            required: jobs.get(FINAL_JOB_ID).map(canonical_job).transpose()?,
            producer_plan: if policy == WorkflowPolicy::VelnorRepositoryV1 {
                jobs.get(PLAN_JOB_ID).map(canonical_job).transpose()?
            } else {
                None
            },
        }];
        authority_jobs.extend(
            expanded_authority_jobs
                .iter()
                .map(|(required, producer_plan)| {
                    Ok(CanonicalAuthorityJobs {
                        required: required.as_ref().map(canonical_job).transpose()?,
                        producer_plan: if policy == WorkflowPolicy::VelnorRepositoryV1 {
                            producer_plan.as_ref().map(canonical_job).transpose()?
                        } else {
                            None
                        },
                    })
                })
                .collect::<Result<Vec<_>, crate::RenderError>>()?,
        );
        let required_shells = jobs
            .get(FINAL_JOB_ID)
            .map_or_else(Vec::new, |job| shell_steps(&job.steps).cloned().collect());
        let producer_plan_shells = (policy == WorkflowPolicy::VelnorRepositoryV1).then(|| {
            jobs.get(PLAN_JOB_ID).map_or_else(Vec::new, |job| {
                shell_steps(plan_prefix(&job.steps)).cloned().collect()
            })
        });
        Ok(Self {
            authority_jobs,
            mise_setup,
            required_shells,
            producer_plan_shells,
            runtime,
        })
    }

    pub(super) fn matches_runtime(&self, context: &crate::render::RenderContext) -> bool {
        self.runtime.plan_consumer_env == context.plan_consumer_env
            && self.runtime.report_helper_version == context.report_helper_version
            && self.runtime.staged_binary == context.staged_binary
            && self.runtime.request_dir == context.request_dir
            && self.runtime.checkout_uses == context.checkout_uses
            && self.runtime.preseed == context.preseed
    }

    pub(super) fn matches_required(&self, steps: &[Step]) -> bool {
        shell_steps(steps).eq(self.required_shells.iter())
    }

    pub(super) fn matches_producer_plan(&self, steps: &[Step]) -> bool {
        self.producer_plan_shells
            .as_ref()
            .is_some_and(|expected| shell_steps(steps).eq(expected.iter()))
    }

    pub(super) fn matches_raw_required(&self, jobs: &BTreeMap<String, Job>) -> bool {
        let actual = match jobs.get(FINAL_JOB_ID) {
            Some(job) => self
                .normalize_optional_setup(job)
                .and_then(|normalized| canonical_job(&normalized).ok())
                .map(Some),
            None => Some(None),
        };
        let Some(actual) = actual else {
            return false;
        };
        self.authority_jobs
            .iter()
            .any(|expected| expected.required == actual)
    }

    pub(super) fn matches_raw_producer_plan(&self, jobs: &BTreeMap<String, Job>) -> bool {
        let required = match jobs.get(FINAL_JOB_ID) {
            Some(job) => self
                .normalize_optional_setup(job)
                .and_then(|normalized| canonical_job(&normalized).ok())
                .map(Some),
            None => Some(None),
        };
        let producer_plan = match jobs.get(PLAN_JOB_ID) {
            Some(job) => self
                .normalize_optional_setup(job)
                .and_then(|normalized| canonical_job(&normalized).ok())
                .map(Some),
            None => Some(None),
        };
        let (Some(required), Some(producer_plan)) = (required, producer_plan) else {
            return false;
        };
        self.authority_jobs.iter().any(|expected| {
            expected.required == required && expected.producer_plan == producer_plan
        })
    }

    fn normalize_optional_setup(&self, job: &Job) -> Option<Job> {
        let mut normalized = job.clone();
        let positions = normalized
            .steps
            .iter()
            .enumerate()
            .filter_map(|(index, step)| (step.role == Some(StepRole::MiseSetup)).then_some(index))
            .collect::<Vec<_>>();
        match positions.as_slice() {
            [] => Some(normalized),
            [index]
                if self
                    .mise_setup
                    .as_ref()
                    .is_some_and(|setup| normalized.steps.get(*index) == Some(setup))
                    && normalized
                        .steps
                        .get(index.wrapping_sub(1))
                        .is_some_and(|step| step.role == Some(StepRole::Checkout)) =>
            {
                normalized.steps.remove(*index);
                Some(normalized)
            }
            _ => None,
        }
    }

    pub(super) fn matches_mise_setup(
        &self,
        setup: &crate::setup::MiseSetup,
    ) -> Result<bool, crate::RenderError> {
        let actual = crate::setup::mise_setup_step(setup)?;
        Ok(self.mise_setup.as_ref() == Some(&actual))
    }
}

fn validate_plan_consumer_env(env: &BTreeMap<String, String>) -> Result<(), crate::RenderError> {
    let expected_fixed = velnor_actions_mise::ISOLATION_ENV
        .into_iter()
        .chain(velnor_actions_mise::NO_AUTO_INSTALL_ENV)
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect::<BTreeMap<_, _>>();
    if expected_fixed
        .iter()
        .any(|(key, value)| env.get(key) != Some(value))
        || env.keys().any(|key| {
            !expected_fixed.contains_key(key.as_str())
                && !crate::toolchain_env::TOOLCHAIN_HOME_KEYS.contains(&key.as_str())
        })
    {
        return Err(crate::RenderError::InvalidWorkflow(
            "trusted_plan_runtime_context".to_owned(),
        ));
    }
    if crate::toolchain_env::TOOLCHAIN_HOME_KEYS
        .iter()
        .any(|key| env.contains_key(*key))
    {
        crate::toolchain_env::check_toolchain_homes(env)?;
    }
    crate::commands::validate_env(env)
}

fn canonical_job(job: &Job) -> Result<Vec<u8>, crate::RenderError> {
    velnor_actions_contract::canonical_json_bytes(job).map_err(|error| {
        crate::RenderError::InvalidWorkflow(format!("authority_job_encoding:{error}"))
    })
}

fn plan_prefix(steps: &[Step]) -> &[Step] {
    let end = steps.iter().position(|step| {
        matches!(&step.kind, StepKind::Internal { operation, .. }
            if operation == crate::steps::PLAN_OPERATION)
    });
    end.map_or(steps, |at| &steps[..=at])
}

fn shell_steps(steps: &[Step]) -> impl Iterator<Item = &Step> {
    steps
        .iter()
        .filter(|step| matches!(&step.kind, StepKind::Shell { .. }))
}
