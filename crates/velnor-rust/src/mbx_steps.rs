// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperCommand, Job, MbxRemote, MbxStore, RepoFacts, Step, StepKind};

use crate::plan::step;

pub(crate) fn apply(facts: &RepoFacts, jobs: &mut [Job]) {
    let remote = facts.config.mbx_remote == MbxRemote::GithubActions;
    let private =
        facts.config.mbx.routed() && !remote && facts.config.mbx_store == MbxStore::Private;
    for job in jobs {
        if !compiles(job) {
            continue;
        }
        if remote && facts.private_sources.is_none() {
            insert(job, &[StepKind::MbxRemote], After::Tools);
        }
        if private {
            insert(job, &[StepKind::MbxPrivate], After::Helper);
        }
    }
}

#[derive(Clone, Copy)]
enum After {
    Tools,
    Helper,
}

fn compiles(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| matches!(&step.kind, StepKind::Cargo(command) if command.builds()))
}

fn insert(job: &mut Job, kinds: &[StepKind], place: After) {
    let Some(at) = job
        .steps
        .iter()
        .position(|step| matches!(step.kind, StepKind::InstallTools { .. }))
    else {
        return;
    };
    let installed = job.steps[at].clone();
    let steps: Vec<Step> = kinds
        .iter()
        .map(|kind| Step {
            gate: installed.gate.clone(),
            pr: installed.pr,
            ..step(kind.clone())
        })
        .collect();
    let index = match place {
        After::Tools => at + 1,
        After::Helper => job
            .steps
            .iter()
            .rposition(|step| {
                matches!(
                    step.kind,
                    StepKind::InstallHelper(_) | StepKind::Helper(HelperCommand::Plan)
                )
            })
            .map_or(at, |helper| (helper + 1).min(at)),
    };
    job.steps.splice(index..index, steps);
}
