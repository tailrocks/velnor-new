// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CacheKey, CargoCommand, Job, PrivateSources, RepoPath, RootSlug, Step, StepKind,
};

use crate::plan::step;

pub(crate) fn insert(sources: &PrivateSources, jobs: &mut [Job]) {
    for job in jobs {
        let Some(first) = job.steps.iter().position(compiles) else {
            continue;
        };
        let mut manifests: Vec<RepoPath> = Vec::new();
        for step in job.steps.iter().filter(|step| compiles(step)) {
            let manifest = manifest(step);
            if !manifests.contains(&manifest) {
                manifests.push(manifest);
            }
        }
        let cached: Vec<RootSlug> = job.steps[..first]
            .iter()
            .filter_map(|step| match &step.kind {
                StepKind::CacheRestore(CacheKey::Sources { root, .. }) => Some(root.slug().clone()),
                _ => None,
            })
            .collect();
        let gate = job.steps[first].gate.clone();
        let phase = [
            StepKind::PrivateToken {
                sources: sources.clone(),
                cached: cached.clone(),
            },
            StepKind::PrivateFetch { manifests, cached },
        ]
        .map(|kind| Step {
            gate: gate.clone(),
            ..step(kind)
        });
        job.steps.splice(first..first, phase);
    }
}

fn manifest(step: &Step) -> RepoPath {
    if let StepKind::Cargo(CargoCommand::Deny { manifest }) = &step.kind {
        return manifest.clone();
    }
    step.directory.as_ref().map_or_else(
        || RepoPath::from_static("Cargo.toml"),
        |directory| directory.child("Cargo.toml"),
    )
}

fn compiles(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Cargo(command) if !matches!(
        command,
        CargoCommand::FmtCheck
            | CargoCommand::Machete
            | CargoCommand::NextestRunArchive { .. }
    ))
}
