// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    Access, FullRun, Job, PermissionScope, Permissions, PrCi, PullRequestType, RepoFacts, Step,
    StepKind, Triggers, When,
};

use crate::plan::{BUDGET_JOB, CHECKOUT_DEPTH, job, step};
use crate::schedule::schedule;

const BUDGET_RUNS: usize = 10;
const BUDGET_TIMEOUT_MINUTES: u16 = 10;

pub(crate) fn budget_job(facts: &RepoFacts) -> Option<Job> {
    if !facts.config.budget.is_some_and(|budget| budget.enabled()) {
        return None;
    }
    let mut steps = vec![step(StepKind::Checkout {
        depth: CHECKOUT_DEPTH,
        sparse: None,
    })];
    steps.extend(
        facts
            .helper
            .iter()
            .map(|release| step(StepKind::InstallHelper(release.clone()))),
    );
    steps.push(step(StepKind::Budget { last: BUDGET_RUNS }));
    Some(Job {
        name: Some("Speed budget".to_owned()),
        when: if facts.config.full_run == FullRun::Off {
            When::DefaultBranchPush
        } else {
            When::Scheduled
        },
        permissions: Permissions(BTreeMap::from([
            (PermissionScope::Actions, Access::Read),
            (PermissionScope::Contents, Access::Read),
        ])),
        timeout_minutes: BUDGET_TIMEOUT_MINUTES,
        steps,
        ..job(facts, BUDGET_JOB)
    })
}

pub(crate) fn triggers(facts: &RepoFacts) -> Triggers {
    Triggers {
        pull_request: PullRequestType::ALL
            .into_iter()
            .chain(
                facts
                    .config
                    .skip_draft_pull_requests
                    .then_some(PullRequestType::ConvertedToDraft),
            )
            .chain((facts.config.pr_ci == PrCi::Label).then_some(PullRequestType::Labeled))
            .collect(),
        push_branch: Some(facts.config.default_branch.clone()),
        merge_group: true,
        schedule: schedule(facts),
        workflow_dispatch: true,
    }
}

pub(crate) fn draft_guard(facts: &RepoFacts) -> Option<Step> {
    match facts.config.pr_ci {
        PrCi::Label => Some(step(StepKind::DraftNotice)),
        PrCi::Always => facts
            .config
            .skip_draft_pull_requests
            .then(|| step(StepKind::FailOnDraftRun)),
    }
}
