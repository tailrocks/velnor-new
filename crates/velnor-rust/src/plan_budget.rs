// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    Access, BUDGET_JOB_NAME, BUDGET_MODES_HELPER, FullRun, GateNeed, Job, JobId, PermissionScope,
    Permissions, PrCi, PullRequestType, RepoFacts, RootSlug, SelectionName, Step, StepKind,
    Triggers, When,
};

use crate::lane::PLAN_JOB;
use crate::plan::{BUDGET_JOB, CHECKOUT_DEPTH, RUST_JOB, job, step};
use crate::schedule::schedule;
use crate::units::Unit;

const BUDGET_RUNS: usize = 10;
const BUDGET_TIMEOUT_MINUTES: u16 = 10;

/// True when the budget job records the selection of its run in its title,
/// which only a helper that reads the title can use.
fn names_selection(facts: &RepoFacts) -> bool {
    facts.config.budget.is_some_and(|budget| budget.enabled())
        && facts
            .helper
            .as_ref()
            .is_some_and(|helper| helper.version().at_least(BUDGET_MODES_HELPER))
}

/// The job whose steps run `velnor-ci plan`.
pub(crate) fn plan_job_id(single: bool) -> JobId {
    if single { RUST_JOB } else { PLAN_JOB }
}

/// Makes the job that runs the plan expose `rust_<root>_mode` of every root, so
/// that the budget job can title itself with what the run selected.
pub(crate) fn expose_selection(facts: &RepoFacts, jobs: &mut [Job], single: bool, units: &[Unit]) {
    if !names_selection(facts) {
        return;
    }
    let plan = plan_job_id(single);
    if let Some(job) = jobs.iter_mut().find(|job| job.id == plan) {
        job.plan_outputs.extend(
            units
                .iter()
                .map(|unit| (unit.gate.clone(), GateNeed::Selected)),
        );
        job.plan_outputs.sort();
        job.plan_outputs.dedup();
    }
}

fn selection_name(facts: &RepoFacts, jobs: &[Job], plan: &JobId) -> Option<SelectionName> {
    if !names_selection(facts) {
        return None;
    }
    let roots: Vec<RootSlug> = jobs
        .iter()
        .find(|job| &job.id == plan)?
        .plan_outputs
        .iter()
        .filter(|(_, need)| *need == GateNeed::Selected)
        .map(|(slug, _)| slug.clone())
        .collect();
    (!roots.is_empty()).then(|| SelectionName {
        plan: plan.clone(),
        roots,
    })
}

pub(crate) fn budget_job(
    facts: &RepoFacts,
    jobs: &[Job],
    single: bool,
    after: &JobId,
) -> Option<Job> {
    if !facts.config.budget.is_some_and(|budget| budget.enabled()) {
        return None;
    }
    let selection = selection_name(facts, jobs, &plan_job_id(single));
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
        name: Some(BUDGET_JOB_NAME.to_owned()),
        needs: std::iter::once(after.clone())
            .chain(selection.as_ref().map(|name| name.plan.clone()))
            .collect(),
        selection_name: selection,
        when: if facts.config.full_run == FullRun::Off {
            When::DefaultBranchPushAfterNeeds
        } else {
            When::ScheduledAfterNeeds
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
                (facts.config.skip_draft_pull_requests || facts.config.pr_ci == PrCi::Label)
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
