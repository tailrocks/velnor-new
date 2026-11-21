// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    Access, CargoCommand, HelperCommand, Job, PermissionScope, PlanFrom, RepoFacts, RepoPath,
    RustRoot, Step, StepKind,
};

use crate::lane::{PLAN_JOB, POLICY_JOB};
use crate::plan::{RUST_JOB, step};
use crate::test_setup;

const PER_ROOT_SETUP_HELPER: (u64, u64, u64) = (0, 1, 112);

pub(crate) fn apply(
    facts: &RepoFacts,
    jobs: &mut [Job],
    single: bool,
    after_merge: &dyn Fn(&StepKind) -> bool,
) {
    let setup_roots = facts
        .helper
        .as_ref()
        .filter(|helper| helper.version().at_least(PER_ROOT_SETUP_HELPER))
        .map(|_| facts.rust_roots.as_slice());
    mark(
        jobs,
        single,
        crate::lane::lane_job_fixes(facts),
        setup_roots,
        after_merge,
    );
}

fn mark(
    jobs: &mut [Job],
    single: bool,
    whole_jobs: bool,
    setup_roots: Option<&[RustRoot]>,
    after_merge: &dyn Fn(&StepKind) -> bool,
) {
    for job in jobs.iter_mut() {
        let from = if !single {
            PlanFrom::Job(PLAN_JOB)
        } else if job.id == RUST_JOB || is_lint_job(job) || plans_itself(job) {
            PlanFrom::ThisJob
        } else if job.needs.contains(&RUST_JOB) {
            PlanFrom::Job(RUST_JOB)
        } else {
            continue;
        };
        let whole =
            whole_jobs && from != PlanFrom::ThisJob && (is_lint_job(job) || job.id == POLICY_JOB);
        if whole {
            skip_whole_job(job, &from, single);
            continue;
        }
        let builds = job
            .steps
            .iter()
            .any(|step| keeps_target_build(&step.kind) || after_merge(&step.kind));
        let keeps: Vec<bool> = job
            .steps
            .iter()
            .map(|step| match setup_roots {
                Some(roots) if matches!(step.kind, StepKind::TestSetup { .. }) => {
                    compiled_by_builds(&job.steps, roots, after_merge, step.directory.as_ref())
                }
                _ => builds,
            })
            .collect();
        for (step, keep) in job.steps.iter_mut().zip(keeps) {
            if step.kind.is_check() && !(keep && step.kind.feeds_build()) {
                step.skip_after_merge_group = Some(from.clone());
            }
        }
        if is_lint_job(job) {
            skip_lint_job(job, &from, single);
        }
        if is_archive_consumer(job) {
            job.skip_after_merge_group = Some(from);
        }
    }
}

fn skip_whole_job(job: &mut Job, from: &PlanFrom, single: bool) {
    if !single && !job.needs.contains(&PLAN_JOB) {
        job.needs.push(PLAN_JOB);
    }
    job.skip_after_merge_group = Some(from.clone());
}

fn compiled_by_builds(
    steps: &[Step],
    roots: &[RustRoot],
    after_merge: &dyn Fn(&StepKind) -> bool,
    directory: Option<&RepoPath>,
) -> bool {
    steps
        .iter()
        .filter(|step| keeps_target_build(&step.kind) || after_merge(&step.kind))
        .any(|build| match &build.manifest {
            None => true,
            Some(manifest) => roots
                .iter()
                .find(|root| &root.manifest == manifest)
                .is_none_or(|root| {
                    test_setup::compiled_dirs(roots, root)
                        .iter()
                        .any(|dir| dir.as_ref() == directory)
                }),
        })
}

fn keeps_target_build(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Cargo(command) if command.is_target_build())
}

fn plans_itself(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| step.kind == StepKind::Helper(HelperCommand::Plan))
}

fn is_archive_consumer(job: &Job) -> bool {
    job.steps.iter().any(|step| {
        matches!(
            step.kind,
            StepKind::Cargo(CargoCommand::NextestRunArchive { .. })
        )
    })
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

pub(crate) fn ask(facts: &RepoFacts, jobs: &mut [Job], single: bool) {
    let required = facts.config.required_check.as_str();
    let asker = if single { RUST_JOB } else { PLAN_JOB };
    for job in jobs
        .iter_mut()
        .filter(|job| job.id == asker || (single && plans_itself(job)))
    {
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
    assert_skips_resolve(jobs);
}

/// `apply` marks steps before `ask` inserts the `merge-group-passed` steps they
/// read. Every mark must name a job that holds that step, or generation stops:
/// a missing step would leave a condition that evaluates empty and silently
/// disables the skip.
fn assert_skips_resolve(jobs: &[Job]) {
    for job in jobs {
        let marks = job.skip_after_merge_group.iter().chain(
            job.steps
                .iter()
                .filter_map(|step| step.skip_after_merge_group.as_ref()),
        );
        for from in marks {
            let reader = match from {
                PlanFrom::ThisJob => Some(job),
                PlanFrom::Job(id) | PlanFrom::WorkJob(id) => jobs.iter().find(|job| &job.id == id),
            };
            let asked = reader.is_some_and(|reader| {
                reader
                    .steps
                    .iter()
                    .any(|step| matches!(step.kind, StepKind::MergeGroupPassed { .. }))
            });
            assert!(
                asked,
                "merge-group skip: {} reads no merge-group-passed step in the job it names",
                job.id
            );
        }
    }
}

#[cfg(test)]
#[path = "merge_group_skip_tests.rs"]
mod tests;
