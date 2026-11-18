// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CargoCommand, Job, PrivateSources, RepoPath, Step, StepKind};

use crate::plan::step;

pub(crate) fn insert(sources: &PrivateSources, jobs: &mut [Job]) {
    for job in jobs {
        let Some(first) = job.steps.iter().position(compiles) else {
            continue;
        };
        let mut manifests: Vec<RepoPath> = Vec::new();
        for step in job.steps.iter().filter(|step| compiles(step)) {
            let manifest = step.directory.as_ref().map_or_else(
                || RepoPath::from_static("Cargo.toml"),
                |directory| directory.child("Cargo.toml"),
            );
            if !manifests.contains(&manifest) {
                manifests.push(manifest);
            }
        }
        let gate = job.steps[first].gate.clone();
        let phase = [
            StepKind::PrivateToken(sources.clone()),
            StepKind::PrivateFetch { manifests },
        ]
        .map(|kind| Step {
            gate: gate.clone(),
            ..step(kind)
        });
        job.steps.splice(first..first, phase);
    }
}

fn compiles(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Cargo(command) if !matches!(
        command,
        CargoCommand::FmtCheck
            | CargoCommand::Deny { .. }
            | CargoCommand::Machete
            | CargoCommand::NextestRunArchive { .. }
    ))
}
