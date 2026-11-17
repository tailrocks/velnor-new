// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{
    CargoCommand, GateNeed, Job, JobCondition, JobId, RepoFacts, RootSlug, RustRoot, RustSelection,
    Step, StepKind, VerdictJob, VerdictSpec, When,
};

use crate::harness::uses_nextest;
use crate::lane::PLAN_JOB;
use crate::plan::{RUST_JOB, job, step};
use crate::required_size::needs_max_bytes;

const VERDICT_JOB: JobId = JobId::from_static("required");

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
        When::Success
        | When::NotCancelled
        | When::Finalizer
        | When::SaveBase
        | When::SaveBaseDecided
        | When::BaseExpired
        | When::SavingRun
        | When::Always
        | When::CacheDebug
        | When::SaveToolsOnDefaultBranch => None,
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

pub(crate) fn gate_outputs(jobs: &[Job]) -> Vec<(RootSlug, GateNeed)> {
    let slugs: BTreeSet<RootSlug> = jobs.iter().filter_map(tests_lane).collect();
    slugs
        .into_iter()
        .map(|slug| (slug, GateNeed::TestsExpected))
        .collect()
}

pub(crate) fn expose_in_single(jobs: &mut [Job]) {
    let outputs = gate_outputs(jobs);
    for job in jobs.iter_mut().filter(|job| job.id == RUST_JOB) {
        job.plan_outputs.clone_from(&outputs);
    }
}

pub(crate) fn verdict_job(
    facts: &RepoFacts,
    jobs: &[Job],
    single: bool,
    draft_guard: Option<Step>,
) -> Job {
    let gate = has_gate(facts);
    let steps: Vec<Step> = if gate {
        let install = facts
            .helper
            .as_ref()
            .map(|release| step(StepKind::InstallHelper(release.clone())));
        let plan = if single { RUST_JOB } else { PLAN_JOB };
        install
            .into_iter()
            .chain([step(StepKind::Verdict {
                spec: spec(plan, jobs),
                needs_max_bytes: needs_max_bytes(facts, jobs),
            })])
            .collect()
    } else {
        vec![step(StepKind::FailWhenANeedFailed)]
    };
    Job {
        name: Some(facts.config.required_check.clone()),
        skip_on_draft: false,
        needs: jobs.iter().map(|job| job.id.clone()).collect(),
        when: if gate {
            When::Always
        } else {
            When::NotCancelled
        },
        steps: draft_guard.into_iter().chain(steps).collect(),
        ..job(facts, VERDICT_JOB)
    }
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
