// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    Access, CargoCommand, Job, PermissionScope, ReleaseAsset, ReleaseAssetPart, RepoFacts, Step,
    StepKind, When,
};

use crate::plan::step;
use crate::private_fetch::{after_trusted_setup, compiles, shared_gate, status, tokens_first};

/// Put the release asset phase in front of the first step that runs checkout code (the first
/// cargo step before helper 0.1.111) of every job that compiles or runs tests, because a build
/// script may read the assets: the restores, the merge queue check, the token, the one step that
/// downloads and verifies and revokes, and the saves, so no repository code runs between the
/// verification and the save. Formatting, `cargo machete` and `cargo deny` run no build script
/// and get nothing.
pub(crate) fn insert(facts: &RepoFacts, jobs: &mut [Job]) {
    let config = &facts.config;
    let (Some(sources), false) = (&config.private_sources, config.release_assets.is_empty()) else {
        return;
    };
    let early = tokens_first(facts);
    for job in jobs {
        let Some(first) = job.steps.iter().position(needs_assets) else {
            continue;
        };
        let (at, when, gate) = if early {
            let at = after_trusted_setup(&job.steps[..first]);
            (
                at,
                When::Success,
                job.steps.get(at).and_then(|step| step.gate.clone()),
            )
        } else {
            (
                first,
                status(job.steps.iter().filter(|step| compiles(step))),
                shared_gate(job.steps.iter().filter(|step| needs_assets(step))),
            )
        };
        let assets = &config.release_assets;
        let parts = restores(assets)
            .chain([
                ReleaseAssetPart::Check,
                ReleaseAssetPart::Token(sources.clone()),
                ReleaseAssetPart::Fetch,
            ])
            .chain(saves(assets));
        let phase: Vec<Step> = parts
            .map(|part| Step {
                gate: gate.clone(),
                when,
                ..step(StepKind::ReleaseAssets {
                    assets: assets.clone(),
                    part,
                })
            })
            .collect();
        job.steps.splice(at..at, phase);
        job.permissions
            .0
            .insert(PermissionScope::PullRequests, Access::Read);
    }
}

fn restores(assets: &[ReleaseAsset]) -> impl Iterator<Item = ReleaseAssetPart> {
    (0..assets.len()).map(ReleaseAssetPart::Restore)
}

fn saves(assets: &[ReleaseAsset]) -> impl Iterator<Item = ReleaseAssetPart> {
    (0..assets.len()).map(ReleaseAssetPart::Save)
}

fn needs_assets(step: &Step) -> bool {
    matches!(step.kind, StepKind::TestSetup { .. })
        || (compiles(step) && !matches!(step.kind, StepKind::Cargo(CargoCommand::Deny { .. })))
        || matches!(
            step.kind,
            StepKind::Cargo(CargoCommand::NextestRunArchive { .. })
        )
}
