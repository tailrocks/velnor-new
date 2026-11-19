// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CargoCommand, RepoFacts, RustRoot, Step, StepKind};

use crate::lane::Lane;
use crate::plan::step;

pub(crate) fn inserted(
    facts: &RepoFacts,
    root: &RustRoot,
    lane: &Lane,
    mut steps: Vec<Step>,
) -> Vec<Step> {
    let Some(first) = steps.iter().position(|step| {
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
    }) else {
        return steps;
    };
    let (when, pr) = (steps[first].when, steps[first].pr);
    let setup: Vec<Step> = facts
        .config
        .test_setup
        .iter()
        .filter(|setup| setup.root == root.manifest.parent())
        .map(|setup| Step {
            directory: root.manifest.parent(),
            gate: lane.steps(),
            when,
            pr,
            ..step(StepKind::TestSetup {
                name: setup.name.clone(),
                run: setup.run.clone(),
            })
        })
        .collect();
    steps.splice(first..first, setup);
    steps
}
