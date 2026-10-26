use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, WorkflowPolicy};

use crate::render::{FINAL_JOB_ID, PLAN_JOB_ID};

#[derive(Debug, Clone)]
pub struct CanonicalAuthoritySteps {
    required: Vec<Step>,
    producer_plan: Option<Vec<Step>>,
}

impl CanonicalAuthoritySteps {
    #[must_use]
    pub fn from_native_jobs(jobs: &BTreeMap<String, Job>, policy: WorkflowPolicy) -> Self {
        let required = jobs
            .get(FINAL_JOB_ID)
            .map_or_else(Vec::new, |job| shell_steps(&job.steps).cloned().collect());
        let producer_plan = (policy == WorkflowPolicy::VelnorRepositoryV1).then(|| {
            jobs.get(PLAN_JOB_ID).map_or_else(Vec::new, |job| {
                let end = job.steps.iter().position(|step| {
                    matches!(&step.kind, StepKind::Internal { operation, .. }
                        if operation == crate::steps::PLAN_OPERATION)
                });
                let prefix = end.map_or(job.steps.as_slice(), |at| &job.steps[..=at]);
                shell_steps(prefix).cloned().collect()
            })
        });
        Self {
            required,
            producer_plan,
        }
    }

    pub(super) fn matches_required(&self, steps: &[Step]) -> bool {
        shell_steps(steps).eq(self.required.iter())
    }

    pub(super) fn matches_producer_plan(&self, steps: &[Step]) -> bool {
        self.producer_plan
            .as_ref()
            .is_some_and(|expected| shell_steps(steps).eq(expected.iter()))
    }
}

fn shell_steps(steps: &[Step]) -> impl Iterator<Item = &Step> {
    steps
        .iter()
        .filter(|step| matches!(&step.kind, StepKind::Shell { .. }))
}
