// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Job, StepKind, WorkflowPlan};

fn own_release(job: &Job) -> Option<&HelperRelease> {
    job.steps.iter().find_map(|step| match &step.kind {
        StepKind::InstallHelper(release) => Some(release),
        _ => None,
    })
}

pub(crate) fn release_of<'a>(job: &'a Job, plan: &'a WorkflowPlan) -> Option<&'a HelperRelease> {
    own_release(job).or_else(|| plan.jobs.iter().find_map(own_release))
}

pub(crate) fn at_least(job: &Job, plan: &WorkflowPlan, version: (u64, u64, u64)) -> bool {
    release_of(job, plan).is_some_and(|release| release.version().at_least(version))
}
