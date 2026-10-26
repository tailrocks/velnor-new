// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, StepRole, WorkflowPolicy};

use crate::render::{FINAL_JOB_ID, PLAN_JOB_ID};

#[derive(Debug, Clone)]
pub struct CanonicalAuthoritySteps {
    required_steps: Option<Vec<Step>>,
    producer_plan_steps: Option<Vec<Step>>,
    producer_plan_expansions: Vec<Vec<Step>>,
    mise_setup: Option<Step>,
    required_shells: Vec<Step>,
    producer_plan_shells: Option<Vec<Step>>,
}

impl CanonicalAuthoritySteps {
    pub fn from_native_jobs_with_mise_setup(
        jobs: &BTreeMap<String, Job>,
        policy: WorkflowPolicy,
        setup: &crate::setup::MiseSetup,
        expanded_plan_jobs: &[Job],
    ) -> Result<Self, crate::RenderError> {
        let setup = crate::setup::mise_setup_step(setup)?;
        Ok(Self::from_native_jobs_inner(
            jobs,
            policy,
            Some(setup),
            expanded_plan_jobs,
        ))
    }

    fn from_native_jobs_inner(
        jobs: &BTreeMap<String, Job>,
        policy: WorkflowPolicy,
        mise_setup: Option<Step>,
        expanded_plan_jobs: &[Job],
    ) -> Self {
        let required_steps = jobs.get(FINAL_JOB_ID).map(|job| job.steps.clone());
        let required_shells = required_steps
            .as_deref()
            .map_or_else(Vec::new, |steps| shell_steps(steps).cloned().collect());
        let producer_plan_steps = if policy == WorkflowPolicy::VelnorRepositoryV1 {
            jobs.get(PLAN_JOB_ID)
                .map(|job| plan_prefix(&job.steps).to_vec())
        } else {
            None
        };
        let producer_plan_shells = (policy == WorkflowPolicy::VelnorRepositoryV1).then(|| {
            producer_plan_steps
                .as_deref()
                .map_or_else(Vec::new, |steps| shell_steps(steps).cloned().collect())
        });
        let producer_plan_expansions = if policy == WorkflowPolicy::VelnorRepositoryV1 {
            expanded_plan_jobs
                .iter()
                .map(|job| plan_prefix(&job.steps).to_vec())
                .collect()
        } else {
            Vec::new()
        };
        Self {
            required_steps,
            producer_plan_steps,
            producer_plan_expansions,
            mise_setup,
            required_shells,
            producer_plan_shells,
        }
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
        match (self.required_steps.as_deref(), jobs.get(FINAL_JOB_ID)) {
            (Some(expected), Some(job)) => {
                matches_with_optional_setup(expected, &job.steps, self.mise_setup.as_ref())
            }
            (None, None) => true,
            _ => false,
        }
    }

    pub(super) fn matches_raw_producer_plan(&self, jobs: &BTreeMap<String, Job>) -> bool {
        match (self.producer_plan_steps.as_deref(), jobs.get(PLAN_JOB_ID)) {
            (Some(expected), Some(job)) => {
                let operations = job
                    .steps
                    .iter()
                    .enumerate()
                    .filter_map(|(index, step)| {
                        matches!(&step.kind, StepKind::Internal { operation, .. }
                            if operation == crate::steps::PLAN_OPERATION)
                        .then_some(index)
                    })
                    .collect::<Vec<_>>();
                if operations.len() != 1 {
                    return false;
                }
                let prefix = &job.steps[..=operations[0]];
                matches_with_optional_setup(expected, prefix, self.mise_setup.as_ref())
                    || self.producer_plan_expansions.iter().any(|candidate| {
                        matches_with_optional_setup(candidate, prefix, self.mise_setup.as_ref())
                    })
            }
            (None, None) => true,
            _ => false,
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

fn plan_prefix(steps: &[Step]) -> &[Step] {
    let end = steps.iter().position(|step| {
        matches!(&step.kind, StepKind::Internal { operation, .. }
            if operation == crate::steps::PLAN_OPERATION)
    });
    end.map_or(steps, |at| &steps[..=at])
}

fn matches_with_optional_setup(expected: &[Step], actual: &[Step], setup: Option<&Step>) -> bool {
    if expected == actual {
        return true;
    }
    let Some(setup) = setup else {
        return false;
    };
    if actual.len() != expected.len() + 1
        || expected
            .iter()
            .filter(|step| step.role == Some(StepRole::Checkout))
            .count()
            != 1
    {
        return false;
    }
    let Some(checkout) = expected
        .iter()
        .position(|step| step.role == Some(StepRole::Checkout))
    else {
        return false;
    };
    let insertion = checkout + 1;
    actual[..insertion] == expected[..insertion]
        && actual[insertion] == *setup
        && actual[insertion + 1..] == expected[insertion..]
}

fn shell_steps(steps: &[Step]) -> impl Iterator<Item = &Step> {
    steps
        .iter()
        .filter(|step| matches!(&step.kind, StepKind::Shell { .. }))
}
