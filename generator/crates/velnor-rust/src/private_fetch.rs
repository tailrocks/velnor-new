// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    Access, CacheKey, CargoCommand, Gate, GateNeed, Job, PermissionScope, PrivateSources, RepoPath,
    RootSlug, Step, StepKind,
};

use crate::plan::step;

pub(crate) fn insert(sources: &PrivateSources, jobs: &mut [Job]) {
    for job in jobs {
        let Some(first) = job.steps.iter().position(compiles) else {
            continue;
        };
        let first = job.steps[..first]
            .iter()
            .position(|step| matches!(step.kind, StepKind::TestSetup { .. }))
            .unwrap_or(first);
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
        let gate = shared_gate(job.steps.iter().filter(|step| runs_repository_code(step)));
        let phase = [
            StepKind::PrivateCheck {
                cached: cached.clone(),
            },
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
        job.permissions
            .0
            .insert(PermissionScope::PullRequests, Access::Read);
    }
}

fn shared_gate<'a>(steps: impl Iterator<Item = &'a Step>) -> Option<Gate> {
    let mut gates = steps.map(|step| step.gate.as_ref());
    let first = gates.next()??;
    let mut shared = first.clone();
    for gate in gates {
        let gate = gate?;
        if *gate == shared {
            continue;
        }
        let selected = |gate: &Gate| {
            gate.slug == shared.slug
                && gate.from == shared.from
                && matches!(
                    gate.need,
                    GateNeed::Selected
                        | GateNeed::Doctests
                        | GateNeed::Docs
                        | GateNeed::PackageArgs
                )
        };
        if !(selected(gate) && selected(&shared)) {
            return None;
        }
        shared = Gate {
            need: GateNeed::Selected,
            package: None,
            ..shared
        };
    }
    Some(shared)
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

fn runs_repository_code(step: &Step) -> bool {
    compiles(step) || matches!(step.kind, StepKind::TestSetup { .. })
}

fn compiles(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Cargo(command) if !matches!(
        command,
        CargoCommand::FmtCheck
            | CargoCommand::Machete
            | CargoCommand::NextestRunArchive { .. }
    ))
}
