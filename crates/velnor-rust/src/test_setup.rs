// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CargoCommand, RepoFacts, RepoPath, RustRoot, Step, StepKind, TestSetup, WorkspaceGroup,
};

use crate::lane::Lane;
use crate::members::Member;
use crate::plan::step;
use crate::private_fetch::{any_gate, shared_gate};

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
    let compiled = compiled_dirs(facts, root);
    let setups = facts
        .config
        .test_setup
        .iter()
        .filter(|setup| setup.before_build())
        .filter(|setup| compiled.contains(&setup.root));
    let Some(first) = steps.iter().position(owned) else {
        return steps;
    };
    let gate = shared_gate(steps.iter().filter(|step| owned(step)));
    let (when, pr) = (steps[first].when, steps[first].pr);
    let mut setup: Vec<Step> = Vec::new();
    for candidate in setups.map(|setup| Step {
        gate: gate.clone(),
        ..setup_step(setup, when, pr)
    }) {
        match steps.iter().position(|step| same_setup(step, &candidate)) {
            Some(index) => {
                steps[index].gate = any_gate(steps[index].gate.as_ref(), candidate.gate.as_ref());
            }
            None => setup.push(candidate),
        }
    }
    let mut at = first;
    while at > 0
        && matches!(
            steps[at - 1].kind,
            StepKind::TestSetup {
                before_build: false,
                ..
            }
        )
    {
        at -= 1;
    }
    steps.splice(at..at, setup);
    steps
}

fn same_setup(step: &Step, candidate: &Step) -> bool {
    matches!(step.kind, StepKind::TestSetup { .. })
        && step.kind == candidate.kind
        && step.directory == candidate.directory
}

fn compiled_dirs(facts: &RepoFacts, root: &RustRoot) -> Vec<Option<RepoPath>> {
    let mut seen: Vec<&RepoPath> = Vec::new();
    let mut pending = vec![root];
    while let Some(next) = pending.pop() {
        if seen.contains(&&next.manifest) {
            continue;
        }
        seen.push(&next.manifest);
        pending.extend(
            facts
                .rust_roots
                .iter()
                .filter(|other| next.depends_on.contains(&other.manifest)),
        );
    }
    seen.into_iter().map(RepoPath::parent).collect()
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
            ..setup_step(setup, when, pr)
        })
        .collect();
    steps.splice(first..first, setup);
    steps
}

const MEMBER_CHECK_HELPER: (u64, u64, u64) = (0, 1, 112);

pub(crate) fn checks_per_member(facts: &RepoFacts) -> bool {
    facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(MEMBER_CHECK_HELPER))
}

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
            builds_or_tests(step) && owns(step, root)
        } else {
            (tests(step) || archives(step)) && owns(step, root)
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

fn owns(step: &Step, root: &RustRoot) -> bool {
    step.manifest
        .as_ref()
        .is_none_or(|manifest| *manifest == root.manifest)
}

fn archives(step: &Step) -> bool {
    matches!(
        step.kind,
        StepKind::Cargo(CargoCommand::NextestArchive { .. })
    )
}

fn setup_step(setup: &TestSetup, when: velnor_model::When, pr: velnor_model::PrRun) -> Step {
    Step {
        directory: setup.root.clone(),
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
