// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{
    CargoCommand, GateNeed, Job, JobCondition, JobId, RepoFacts, RootSlug, RustRoot, RustSelection,
    Step, StepKind, VerdictJob, VerdictSpec, When,
};

use crate::harness::uses_nextest;

pub(crate) const VERDICT_HELPER: (u64, u64, u64) = (0, 1, 81);

pub(crate) fn has_gate(facts: &RepoFacts) -> bool {
    facts
        .helper
        .as_ref()
        .is_some_and(|release| release.version().at_least(VERDICT_HELPER))
}

fn tests_lane(job: &Job) -> Option<RootSlug> {
    job.steps.iter().find_map(|step| match &step.kind {
        StepKind::Cargo(
            CargoCommand::NextestRun { .. } | CargoCommand::NextestRunArchive { .. },
        ) => step.reads.as_ref().map(|plan| plan.slug.clone()),
        _ => None,
    })
}

fn conditions(job: &Job) -> Vec<JobCondition> {
    let gate = job.gate.as_ref().and_then(|gate| match gate.need {
        GateNeed::Selected => Some(JobCondition::Lane(gate.slug.clone())),
        GateNeed::Policy => Some(JobCondition::Policy),
        _ => None,
    });
    let when = match job.when {
        When::Scheduled => Some(JobCondition::Schedule),
        When::ScheduledOrPush => Some(JobCondition::ScheduleOrPush),
        When::Success | When::NotCancelled | When::Always => None,
    };
    gate.into_iter().chain(when).collect()
}

pub(crate) fn spec(plan: JobId, jobs: &[Job]) -> VerdictSpec {
    VerdictSpec {
        plan,
        jobs: jobs
            .iter()
            .map(|job| VerdictJob {
                id: job.id.clone(),
                when: conditions(job),
                tests: tests_lane(job),
            })
            .collect(),
    }
}

pub(crate) fn plan_outputs(jobs: &[Job]) -> Vec<(RootSlug, GateNeed)> {
    let slugs: BTreeSet<RootSlug> = jobs.iter().filter_map(tests_lane).collect();
    slugs
        .into_iter()
        .map(|slug| (slug, GateNeed::TestsExpected))
        .collect()
}

pub(crate) fn verdict_step(spec: VerdictSpec) -> Step {
    crate::plan::step(StepKind::Verdict(spec))
}

#[must_use]
pub fn tests_expected(facts: &RepoFacts, root: &RustRoot, selection: &RustSelection) -> bool {
    root.members.iter().any(|member| match selection {
        RustSelection::None => false,
        RustSelection::All(_) => uses_nextest(&facts.config, member),
        RustSelection::Some(packages) => {
            packages.contains_key(&member.name) && uses_nextest(&facts.config, member)
        }
    })
}
