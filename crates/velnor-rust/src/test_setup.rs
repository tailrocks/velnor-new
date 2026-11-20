// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CargoCommand, RepoFacts, RepoPath, RustRoot, Step, StepKind, TestSetup, WorkspaceGroup,
};

use crate::lane::Lane;
use crate::members::Member;
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
    let setups = |build: bool| {
        facts
            .config
            .test_setup
            .iter()
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
    if let Some(first) = steps.iter().position(tests) {
        let (when, pr) = (steps[first].when, steps[first].pr);
        let setup: Vec<Step> = setups(false)
            .map(|setup| Step {
                gate: lane.steps(),
                ..setup_step(root, setup, when, pr)
            })
            .collect();
        steps.splice(first..first, setup);
    }
    steps
}

/// Puts the `[[check_configurations]]` steps of each member in the job, before the member's first
/// test step, or before its first compiling step when the member has no test command.
pub(crate) fn checked_members(
    facts: &RepoFacts,
    members: &[Member],
    mut steps: Vec<Step>,
    checks: bool,
) -> Vec<Step> {
    if !checks {
        return steps;
    }
    for member in members {
        steps = checked(
            facts,
            &member.root,
            &member.lane,
            steps,
            member.tests(facts).is_empty(),
        );
    }
    steps
}

pub(crate) fn checked(
    facts: &RepoFacts,
    root: &RustRoot,
    lane: &Lane,
    mut steps: Vec<Step>,
    untested: bool,
) -> Vec<Step> {
    let place = |step: &Step| {
        if untested {
            builds_or_tests(step)
        } else {
            tests(step) || archives(step)
        }
    };
    let Some(first) = steps.iter().position(place) else {
        return steps;
    };
    if matches!(
        steps[first].kind,
        StepKind::Cargo(CargoCommand::NextestRunArchive { .. })
    ) {
        return steps;
    }
    let (when, pr) = (steps[first].when, steps[first].pr);
    let target = group_target(facts, root);
    let checks: Vec<Step> = facts
        .config
        .check_configurations
        .iter()
        .filter(|check| check.root == root.manifest.parent())
        .map(|check| {
            setup_step(
                root,
                &TestSetup {
                    root: check.root.clone(),
                    name: "velnor-ci check-configurations".to_owned(),
                    run: check.command(),
                    phase: None,
                },
                when,
                pr,
            )
        })
        .map(|check| Step {
            gate: lane.steps(),
            target_dir: target.clone(),
            ..check
        })
        .collect();
    steps.splice(first..first, checks);
    steps
}

fn group_target(facts: &RepoFacts, root: &RustRoot) -> Option<RepoPath> {
    facts
        .config
        .workspace_groups
        .iter()
        .find(|group: &&WorkspaceGroup| group.roots.contains(&root.manifest))
        .map(WorkspaceGroup::target_dir)
}

fn archives(step: &Step) -> bool {
    matches!(
        step.kind,
        StepKind::Cargo(CargoCommand::NextestArchive { .. })
    )
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
