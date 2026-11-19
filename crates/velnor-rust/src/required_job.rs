// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{
    CargoCommand, GateNeed, Job, JobCondition, JobId, PrCi, PrRun, RepoFacts, RootSlug, RustRoot,
    RustSelection, Step, StepKind, VerdictJob, VerdictSpec, When,
};

use crate::harness::uses_nextest;
use crate::lane::PLAN_JOB;
use crate::plan::{RUST_JOB, job, step};
use crate::required_size::needs_max_bytes;

pub(crate) const VERDICT_JOB: JobId = JobId::from_static("required");

fn tests_lane(job: &Job) -> Vec<RootSlug> {
    job.steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::Cargo(
                CargoCommand::NextestRun { .. } | CargoCommand::NextestRunArchive { .. },
            ) => step.reads.as_ref().map(|plan| plan.slug.clone()),
            _ => None,
        })
        .collect()
}

fn conditions(job: &Job) -> Vec<JobCondition> {
    let gate: Vec<JobCondition> = job
        .gate
        .iter()
        .flat_map(|gate| match gate.need {
            GateNeed::Selected => vec![JobCondition::Lane(gate.slug.clone())],
            GateNeed::Group => std::iter::once(JobCondition::Lane(gate.slug.clone()))
                .chain(
                    gate.group
                        .iter()
                        .map(|name| JobCondition::Group(gate.slug.clone(), name.clone())),
                )
                .collect(),
            GateNeed::Policy => vec![JobCondition::Policy],
            _ => Vec::new(),
        })
        .collect();
    let when = match job.when {
        When::FullAudit => Some(JobCondition::ScheduleOrDispatch),
        When::FullAuditOrPush => Some(JobCondition::ScheduleOrPushOrDispatch),
        When::Scheduled | When::ScheduledAfterNeeds => Some(JobCondition::Schedule),
        When::ScheduledOrPush => Some(JobCondition::ScheduleOrPush),
        When::DefaultBranchPush | When::DefaultBranchPushAfterNeeds => {
            Some(JobCondition::DefaultBranchPush)
        }
        When::Success
        | When::NotCancelled
        | When::NotCancelledPostMerge
        | When::Finalizer
        | When::SaveBase
        | When::SaveBaseDecided
        | When::SaveSources
        | When::PullRequest
        | When::SaveDelta
        | When::SaveDeltaDecided
        | When::FetchSources
        | When::BaseExpired
        | When::SavingRun
        | When::Always
        | When::CacheDebug
        | When::ClearRunLabel
        | When::RestoreToolsUnlessLocal
        | When::SaveToolsUnlessLocal
        | When::PrimeToolsFromNetwork
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
    let slugs: BTreeSet<RootSlug> = jobs.iter().flat_map(tests_lane).collect();
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
    let on_label = facts.config.pr_ci == PrCi::Label;
    let strict = if on_label {
        PrRun::Requested
    } else {
        PrRun::Always
    };
    let install = facts.helper.as_ref().map(|release| Step {
        pr: strict,
        ..step(StepKind::InstallHelper(release.clone()))
    });
    let plan = if single { RUST_JOB } else { PLAN_JOB };
    let steps: Vec<Step> = install
        .into_iter()
        .chain([Step {
            pr: strict,
            ..step(StepKind::Verdict {
                spec: spec(plan, jobs),
                needs_max_bytes: needs_max_bytes(facts, jobs),
            })
        }])
        .chain(on_label.then(|| Step {
            pr: PrRun::Unrequested,
            ..step(StepKind::FailWhenANeedFailed)
        }))
        .collect();
    Job {
        name: Some(facts.config.required_check.clone()),
        pr: if facts.config.pr_ci == PrCi::Label {
            PrRun::Related
        } else {
            PrRun::Always
        },
        needs: jobs.iter().map(|job| job.id.clone()).collect(),
        when: When::Always,
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
