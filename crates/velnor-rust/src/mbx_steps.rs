// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperCommand, Job, MbxRemote, MbxStats, MbxStore, RepoFacts, Step, StepKind};

use crate::mbx_pin::{MBX_PIN_HELPER, MBX_TOOL, exact_release};
use crate::plan::step;

pub(crate) const MBX_OUT_DIR_HELPER: (u64, u64, u64) = (0, 1, 111);
const MBX_REPORTS_HELPER: (u64, u64, u64) = (0, 1, 111);
const MBX_REPORTS_RELEASE: (u64, u64, u64) = (1, 30, 0);

pub(crate) fn apply(facts: &RepoFacts, jobs: &mut [Job]) {
    let remote = facts.config.mbx_remote == MbxRemote::GithubActions;
    let from_pin = facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(MBX_PIN_HELPER));
    let private =
        facts.config.mbx.routed() && !remote && facts.config.mbx_store == MbxStore::Private;
    let out_dirs_in_target = facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(MBX_OUT_DIR_HELPER));
    let stats = if facts.config.mbx.routed()
        && facts
            .helper
            .as_ref()
            .is_some_and(|helper| helper.version().at_least(MBX_REPORTS_HELPER))
        && !pinned_before_reports(facts)
    {
        MbxStats::Reports
    } else {
        MbxStats::Summary
    };
    for job in jobs {
        if !compiles(job) {
            continue;
        }
        job.mbx_stats = stats;
        if remote && facts.private_sources.is_none() {
            let remote_steps = if from_pin {
                vec![StepKind::MbxPin, StepKind::MbxRemote { from_pin }]
            } else {
                vec![StepKind::MbxRemote { from_pin }]
            };
            insert(
                job,
                &with_out_dirs(remote_steps, out_dirs_in_target),
                After::Tools,
            );
        }
        if private {
            insert(
                job,
                &with_out_dirs(vec![StepKind::MbxPrivate], out_dirs_in_target),
                After::Helper,
            );
        }
    }
}

fn with_out_dirs(mut kinds: Vec<StepKind>, in_target: bool) -> Vec<StepKind> {
    if in_target {
        kinds.push(StepKind::MbxOutDirs);
    }
    kinds
}

fn pinned_before_reports(facts: &RepoFacts) -> bool {
    facts.tools.tools.get(MBX_TOOL).is_some_and(|pin| {
        pin.requested.iter().any(|request| {
            exact_release(&request.version).is_some_and(|text| {
                let mut parts = text.split('.').filter_map(|part| part.parse::<u64>().ok());
                let release = (
                    parts.next().unwrap_or(0),
                    parts.next().unwrap_or(0),
                    parts.next().unwrap_or(0),
                );
                release < MBX_REPORTS_RELEASE
            })
        })
    })
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
