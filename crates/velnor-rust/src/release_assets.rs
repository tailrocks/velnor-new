// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    Access, CargoCommand, Job, PermissionScope, ReleaseAsset, ReleaseAssetPart, RustCiConfig, Step,
    StepKind,
};

use crate::plan::step;
use crate::private_fetch::{compiles, shared_gate};

/// Put the release asset phase in front of the first cargo step of every job that compiles or
/// runs tests, because a build script may read the assets: the restores, the merge queue check,
/// the token, the one step that downloads and verifies and revokes, and the saves, so no
/// repository code runs between the verification and the save. Formatting, `cargo machete` and
/// `cargo deny` run no build script and get nothing.
pub(crate) fn insert(config: &RustCiConfig, jobs: &mut [Job]) {
    let (Some(sources), false) = (&config.private_sources, config.release_assets.is_empty()) else {
        return;
    };
    for job in jobs {
        let Some(first) = job.steps.iter().position(needs_assets) else {
            continue;
        };
        let gate = shared_gate(job.steps.iter().filter(|step| needs_assets(step)));
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
                ..step(StepKind::ReleaseAssets {
                    assets: assets.clone(),
                    part,
                })
            })
            .collect();
        job.steps.splice(first..first, phase);
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
