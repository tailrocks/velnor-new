// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    Access, CargoCommand, HelperCommand, Job, PermissionScope, PlanFrom, RepoFacts, StepKind,
};

use crate::lane::PLAN_JOB;
use crate::plan::{RUST_JOB, step};

fn is_check(kind: &StepKind) -> bool {
    matches!(
        kind,
        StepKind::TestSetup { .. }
            | StepKind::Cargo(
                CargoCommand::FmtCheck
                    | CargoCommand::Clippy { .. }
                    | CargoCommand::ClippyFeatures { .. }
                    | CargoCommand::Doc
                    | CargoCommand::DocFeatures { .. }
                    | CargoCommand::CheckAlone { .. }
                    | CargoCommand::NextestRun { .. }
                    | CargoCommand::NextestFeatures { .. }
                    | CargoCommand::LaneGuard { .. }
                    | CargoCommand::NextestRunArchive { .. }
                    | CargoCommand::TestTarget { .. }
                    | CargoCommand::Doctest
                    | CargoCommand::Deny { .. }
                    | CargoCommand::Machete
            )
    )
}

/// Lets the lint and test steps skip when `skip_push` says a `merge_group` run
/// of this workflow already passed on this commit. Builds and cache saves keep
/// running.
pub(crate) fn apply(jobs: &mut [Job], single: bool) {
    for job in jobs.iter_mut() {
        let from = if !single {
            PlanFrom::Job(PLAN_JOB)
        } else if job.id == RUST_JOB || is_lint_job(job) {
            PlanFrom::ThisJob
        } else if job.needs.contains(&RUST_JOB) {
            PlanFrom::Job(RUST_JOB)
        } else {
            continue;
        };
        for step in job.steps.iter_mut().filter(|step| is_check(&step.kind)) {
            step.skip_after_merge_group = Some(from.clone());
        }
        if is_lint_job(job) {
            skip_lint_job(job, &from, single);
        }
    }
}

fn is_lint_job(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::Lint(_)))
}

fn skip_lint_job(job: &mut Job, from: &PlanFrom, single: bool) {
    if !single && !job.needs.contains(&PLAN_JOB) {
        job.needs.push(PLAN_JOB);
    }
    for step in &mut job.steps {
        step.skip_after_merge_group = Some(from.clone());
    }
}

/// Asks the helper, right after the plan, whether a `merge_group` run of this
/// workflow already passed on this commit. `required` names the job of that run
/// whose success counts while the run is still going.
pub(crate) fn ask(facts: &RepoFacts, jobs: &mut [Job], single: bool) {
    let required = facts.config.required_check.as_str();
    let asker = if single { RUST_JOB } else { PLAN_JOB };
    for job in jobs.iter_mut().filter(|job| job.id == asker) {
        if let Some(at) = job
            .steps
            .iter()
            .position(|step| step.kind == StepKind::Helper(HelperCommand::Plan))
        {
            job.steps.insert(
                at + 1,
                step(StepKind::MergeGroupPassed {
                    required: required.to_owned(),
                }),
            );
            job.permissions
                .0
                .insert(PermissionScope::Actions, Access::Read);
        }
    }
    if let Some(release) = facts.helper.as_ref().filter(|_| single) {
        for job in jobs.iter_mut().filter(|job| is_lint_job(job)) {
            job.steps.splice(
                0..0,
                [
                    step(StepKind::InstallHelper(release.clone())),
                    step(StepKind::MergeGroupPassed {
                        required: required.to_owned(),
                    }),
                ],
            );
            job.permissions
                .0
                .insert(PermissionScope::Actions, Access::Read);
        }
    }
}
