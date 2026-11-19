// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    Access, CargoCommand, HelperCommand, Job, PermissionScope, PlanFrom, RepoFacts, StepKind,
};

use crate::lane::PLAN_JOB;
use crate::plan::{RUST_JOB, step};

fn is_check(kind: &StepKind) -> bool {
    match kind {
        StepKind::TestSetup { .. } => true,
        StepKind::Cargo(command) => is_check_command(command),
        _ => false,
    }
}

/// Exhaustive, so a new command must be classified here when it is added.
fn is_check_command(command: &CargoCommand) -> bool {
    match command {
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
        | CargoCommand::Machete => true,
        CargoCommand::BuildBinary { .. }
        | CargoCommand::BuildAll
        | CargoCommand::NextestBuild
        | CargoCommand::NextestArchive { .. } => false,
    }
}

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
        if is_archive_consumer(job) {
            job.skip_after_merge_group = Some(from);
        }
    }
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
    assert_skips_resolve(jobs);
}

/// `apply` marks steps before `ask` inserts the `merge-group-passed` steps they
/// read. Every mark must name a job that holds that step, or generation stops:
/// a missing step would leave a condition that evaluates empty and silently
/// disables the skip.
fn assert_skips_resolve(jobs: &[Job]) {
    for job in jobs {
        for step in &job.steps {
            let Some(from) = &step.skip_after_merge_group else {
                continue;
            };
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
mod tests {
    use std::collections::BTreeMap;

    use velnor_model::{
        CargoCommand, FilterRef, Job, JobId, PackageName, Permissions, PrRun, RunnerLabel,
        StepKind, TargetName, When,
    };

    use super::*;

    fn job(id: JobId, kinds: Vec<StepKind>) -> Job {
        Job {
            id,
            name: None,
            comment: None,
            needs: Vec::new(),
            when: When::Success,
            pr: PrRun::Always,
            runs_on: RunnerLabel::from_static("ubuntu-latest"),
            permissions: Permissions(BTreeMap::new()),
            timeout_minutes: 30,
            gate: None,
            skip_after_merge_group: None,
            plan_outputs: Vec::new(),
            group_outputs: Vec::new(),
            reports_ignored: false,
            selection_name: None,
            steps: kinds.into_iter().map(step).collect(),
        }
    }

    #[test]
    fn the_checks_and_only_the_checks_are_classified_as_checks() {
        let checks = [
            CargoCommand::FmtCheck,
            CargoCommand::Clippy { extra: Vec::new() },
            CargoCommand::Doc,
            CargoCommand::Doctest,
            CargoCommand::Machete,
            CargoCommand::NextestRun {
                filter: FilterRef::All,
                ci_profile: true,
            },
        ];
        let builds = [
            CargoCommand::BuildAll,
            CargoCommand::NextestBuild,
            CargoCommand::BuildBinary {
                package: PackageName::from_static("app"),
                binary: TargetName::from_static("app"),
            },
        ];
        for command in checks {
            assert!(is_check_command(&command), "{command:?}");
        }
        for command in builds {
            assert!(!is_check_command(&command), "{command:?}");
        }
    }

    #[test]
    #[should_panic(expected = "reads no merge-group-passed step")]
    fn a_skip_naming_a_missing_merge_group_step_stops_generation() {
        let mut marked = step(StepKind::Cargo(CargoCommand::FmtCheck));
        marked.skip_after_merge_group = Some(PlanFrom::ThisJob);
        let mut jobs = [job(RUST_JOB, vec![StepKind::Cargo(CargoCommand::Doc)])];
        jobs[0].steps.push(marked);
        assert_skips_resolve(&jobs);
    }
}
