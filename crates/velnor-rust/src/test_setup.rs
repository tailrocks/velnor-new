// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CargoCommand, RepoFacts, RustRoot, Step, StepKind, TestSetup};

use crate::lane::Lane;
use crate::plan::step;
use crate::private_fetch::shared_gate;

/// Puts each `[[test_setup]]` step of `root` into `steps`. A step with `phase = "build"` goes
/// before the first cargo step that compiles or runs tests, so a build script of any job can
/// read what it makes. Every other step goes before the first test step. The build-phase steps
/// sit directly in front of repository code, after every step that precedes the first cargo
/// step (the private-sources token phase goes in front of them, see `private_fetch`), so no
/// token or verdict step follows repository code that it did not already follow.
pub(crate) fn inserted(
    facts: &RepoFacts,
    root: &RustRoot,
    lane: &Lane,
    mut steps: Vec<Step>,
) -> Vec<Step> {
    let all = all_setups(facts);
    let setups = |build: bool| {
        all.iter()
            .filter(move |setup| setup.before_build() == build)
            .filter(|setup| setup.root == root.manifest.parent())
    };
    if let Some(first) = steps.iter().position(builds_or_tests) {
        let gate = shared_gate(steps.iter().filter(|step| builds_or_tests(step)));
        let (when, pr) = (steps[first].when, steps[first].pr);
        let setup: Vec<Step> = setups(true)
            .map(|setup| Step {
                gate: gate.clone(),
                ..setup_step(root, setup, when, pr)
            })
            .collect();
        steps.splice(first..first, setup);
    }
    let Some(first) = steps.iter().position(tests) else {
        return steps;
    };
    let (when, pr) = (steps[first].when, steps[first].pr);
    let setup: Vec<Step> = setups(false)
        .map(|setup| Step {
            gate: lane.steps(),
            ..setup_step(root, setup, when, pr)
        })
        .collect();
    steps.splice(first..first, setup);
    steps
}

/// The `[[test_setup]]` entries, then one entry for each `[[check_configurations]]`, which runs
/// the same way: before the tests of its root, as repository code.
fn all_setups(facts: &RepoFacts) -> Vec<TestSetup> {
    let checks = facts
        .config
        .check_configurations
        .iter()
        .map(|check| TestSetup {
            root: check.root.clone(),
            name: "velnor-ci check-configurations".to_owned(),
            run: check.command(),
            phase: None,
        });
    facts
        .config
        .test_setup
        .iter()
        .cloned()
        .chain(checks)
        .collect()
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

/// A cargo step that compiles the package or runs its tests. Formatting, cargo-machete and
/// cargo-deny read no build script, so a job with only those has no build-phase step.
fn builds_or_tests(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Cargo(command) if command.builds()
        || matches!(command, CargoCommand::NextestRunArchive { .. }))
}
