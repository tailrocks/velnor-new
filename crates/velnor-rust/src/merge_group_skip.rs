// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Access, CargoCommand, HelperCommand, Job, PermissionScope, PlanFrom, StepKind};

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
            )
    )
}

/// Asks the helper, right after the plan, whether a `merge_group` run of this
/// workflow already passed on this commit, and lets the lint and test steps
/// skip when it did. Builds and cache saves keep running.
pub(crate) fn apply(jobs: &mut [Job], single: bool) {
    let (asker, from) = if single {
        (RUST_JOB, PlanFrom::ThisJob)
    } else {
        (PLAN_JOB, PlanFrom::Job(PLAN_JOB))
    };
    for job in jobs.iter_mut() {
        if job.id == asker
            && let Some(at) = job
                .steps
                .iter()
                .position(|step| step.kind == StepKind::Helper(HelperCommand::Plan))
        {
            job.steps.insert(
                at + 1,
                step(StepKind::Helper(HelperCommand::MergeGroupPassed)),
            );
            job.permissions
                .0
                .insert(PermissionScope::Actions, Access::Read);
        }
        for step in job.steps.iter_mut().filter(|step| is_check(&step.kind)) {
            step.skip_after_merge_group = Some(from.clone());
        }
    }
}
