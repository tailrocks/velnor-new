// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Access, CargoCommand, HelperCommand, Job, PermissionScope, PlanFrom, StepKind};

use crate::lane::PLAN_JOB;
use crate::plan::{RUST_JOB, step};

fn is_check(kind: &StepKind) -> bool {
    match kind {
        StepKind::TestSetup { .. } => true,
        StepKind::Cargo(command) => is_check_command(command),
        _ => false,
    }
}

/// A lint or test command. Builds, archives and cache saves keep running.
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

/// Lets the lint and test steps skip when `skip_push` says a `merge_group` run
/// of this workflow already passed on this commit. Builds and cache saves keep
/// running.
pub(crate) fn apply(jobs: &mut [Job], single: bool) {
    for job in jobs.iter_mut() {
        let from = if !single {
            PlanFrom::Job(PLAN_JOB)
        } else if job.id == RUST_JOB {
            PlanFrom::ThisJob
        } else if job.needs.contains(&RUST_JOB) {
            PlanFrom::Job(RUST_JOB)
        } else {
            continue;
        };
        for step in job.steps.iter_mut().filter(|step| is_check(&step.kind)) {
            step.skip_after_merge_group = Some(from.clone());
        }
        if !single && is_lint_job(job) {
            skip_lint_job(job, &from);
        }
    }
}

fn is_lint_job(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::Lint(_)))
}

fn skip_lint_job(job: &mut Job, from: &PlanFrom) {
    if !job.needs.contains(&PLAN_JOB) {
        job.needs.push(PLAN_JOB);
    }
    for step in &mut job.steps {
        step.skip_after_merge_group = Some(from.clone());
    }
}

/// Asks the helper, right after the plan, whether a `merge_group` run of this
/// workflow already passed on this commit. `required` names the job of that run
/// whose success counts while the run is still going.
pub(crate) fn ask(jobs: &mut [Job], single: bool, required: &str) {
    let asker = if single { RUST_JOB } else { PLAN_JOB };
    let mut inserted = false;
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
            inserted = true;
        }
    }
    // `apply` has already marked the checks, so a missing asker must stop the
    // generation rather than leave conditions that name a step that is not there.
    assert!(
        inserted,
        "merge-group skip: no {asker} job has a plan step to ask the helper after"
    );
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
            plan_outputs: Vec::new(),
            group_outputs: Vec::new(),
            reports_ignored: false,
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
    #[should_panic(expected = "plan step to ask")]
    fn skipping_refuses_a_shape_where_the_asker_step_was_not_inserted() {
        let mut jobs = [job(RUST_JOB, vec![StepKind::Cargo(CargoCommand::FmtCheck)])];
        ask(&mut jobs, true, "required");
    }
}
