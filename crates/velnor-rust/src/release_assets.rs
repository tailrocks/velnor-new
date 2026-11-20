// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    Access, CargoCommand, Job, PermissionScope, ReleaseAsset, ReleaseAssetPart, RustCiConfig,
    Step, StepKind,
};

use crate::plan::step;
use crate::private_fetch::shared_gate;

/// Put the release asset phase in front of the first test setup step or test step of every job
/// that runs tests: the restores, the merge queue check, the token, the one step that downloads
/// and verifies and revokes, and the saves, so no test code runs between the verification and
/// the save.
pub(crate) fn insert(config: &RustCiConfig, jobs: &mut [Job]) {
    let (Some(sources), false) = (&config.private_sources, config.release_assets.is_empty()) else {
        return;
    };
    for job in jobs {
        let Some(first_test) = job.steps.iter().position(runs_tests) else {
            continue;
        };
        let first = job.steps[..first_test]
            .iter()
            .position(|step| matches!(step.kind, StepKind::TestSetup { .. }))
            .unwrap_or(first_test);
        let (when, pr) = (job.steps[first_test].when, job.steps[first_test].pr);
        let gate = shared_gate(job.steps.iter().filter(|step| runs_tests(step)));
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
                pr,
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

fn runs_tests(step: &Step) -> bool {
    matches!(
        step.kind,
        StepKind::Cargo(
            CargoCommand::NextestRun { .. }
                | CargoCommand::NextestFeatures { .. }
                | CargoCommand::NextestRunArchive { .. }
                | CargoCommand::TestTarget { .. }
                | CargoCommand::Doctest
        )
    )
}
