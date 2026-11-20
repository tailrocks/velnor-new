// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Job, WorkflowPlan};

use crate::rules::RenderedTrusted;

pub(crate) fn fork_runner(job: &Job, plan: &WorkflowPlan) -> Option<String> {
    (!plan.hosted_runners.contains(&job.runs_on)).then(|| plan.fork_runner.to_string())
}

pub(crate) fn trusted_runner(job: &Job, plan: &WorkflowPlan) -> Option<RenderedTrusted> {
    plan.trusted_runners
        .for_runner(&job.runs_on)
        .map(|trusted| RenderedTrusted {
            label: trusted.to_string(),
            replaces: job.runs_on.to_string(),
        })
}
