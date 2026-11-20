// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CargoCommand, RepoFacts, RustRoot, Step, StepKind, TestSetup};

use crate::lane::Lane;
use crate::plan::step;
use crate::private_fetch::shared_gate;

pub(crate) fn inserted(
    facts: &RepoFacts,
    root: &RustRoot,
    lane: &Lane,
    steps: Vec<Step>,
) -> Vec<Step> {
    let steps = before_builds(facts, root, steps);
    before_tests(facts, root, lane, steps)
}

pub(crate) fn before_builds(facts: &RepoFacts, root: &RustRoot, mut steps: Vec<Step>) -> Vec<Step> {
    let owned = |step: &Step| builds_or_tests(step) && owns(step, root);
    let setups = facts
        .config
        .test_setup
        .iter()
        .filter(|setup| setup.before_build())
        .filter(|setup| setup.root == root.manifest.parent());
    let Some(first) = steps.iter().position(owned) else {
        return steps;
    };
    let gate = shared_gate(steps.iter().filter(|step| owned(step)));
    let (when, pr) = (steps[first].when, steps[first].pr);
    let setup: Vec<Step> = setups
        .map(|setup| Step {
            gate: gate.clone(),
            ..setup_step(root, setup, when, pr)
        })
        .collect();
    steps.splice(first..first, setup);
    steps
}

pub(crate) fn before_tests(
    facts: &RepoFacts,
    root: &RustRoot,
    lane: &Lane,
    mut steps: Vec<Step>,
) -> Vec<Step> {
    let Some(first) = steps.iter().position(tests) else {
        return steps;
    };
    let (when, pr) = (steps[first].when, steps[first].pr);
    let setup: Vec<Step> = facts
        .config
        .test_setup
        .iter()
        .filter(|setup| !setup.before_build())
        .filter(|setup| setup.root == root.manifest.parent())
        .map(|setup| Step {
            gate: lane.steps(),
            ..setup_step(root, setup, when, pr)
        })
        .collect();
    steps.splice(first..first, setup);
    steps
}

fn owns(step: &Step, root: &RustRoot) -> bool {
    step.manifest
        .as_ref()
        .is_none_or(|manifest| *manifest == root.manifest)
}

fn setup_step(
    root: &RustRoot,
    setup: &TestSetup,
    when: velnor_model::When,
    pr: velnor_model::PrRun,
) -> Step {
    Step {
        directory: root.manifest.parent(),
        when,
        pr,
        ..step(StepKind::TestSetup {
            name: setup.name.clone(),
            run: setup.run.clone(),
            before_build: setup.before_build(),
        })
    }
}

fn tests(step: &Step) -> bool {
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

fn builds_or_tests(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Cargo(command) if command.builds()
        || matches!(command, CargoCommand::NextestRunArchive { .. }))
}
