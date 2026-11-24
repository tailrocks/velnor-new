// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CargoCommand, RepoFacts, RepoPath, RustRoot, SetupOrder, SetupPhase, Step, StepKind, TestSetup,
    WorkspaceGroup,
};

use crate::lane::Lane;
use crate::members::Member;
use crate::plan::step;
use crate::private_fetch::{any_gate, shared_gate};

// helper-gate: first in tools-v0.1.112 via 8843633902
const LANE_GUARD_ORDER_HELPER: (u64, u64, u64) = (0, 1, 112);

fn lane_guard_ordered(facts: &RepoFacts) -> bool {
    facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(LANE_GUARD_ORDER_HELPER))
}

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
    let compiled = compiled_dirs(&facts.rust_roots, root);
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
        setup_order: SetupOrder::AfterSetup,
        ..setup_step(setup, when, pr, false)
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

pub(crate) fn compiled_dirs(roots: &[RustRoot], root: &RustRoot) -> Vec<Option<RepoPath>> {
    let mut seen: Vec<&RepoPath> = Vec::new();
    let mut pending = vec![root];
    while let Some(next) = pending.pop() {
        if seen.contains(&&next.manifest) {
            continue;
        }
        seen.push(&next.manifest);
        pending.extend(
            roots
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
            setup_order: SetupOrder::AfterSetup,
            ..setup_step(setup, when, pr, false)
        })
        .collect();
    steps.splice(first..first, setup);
    steps
}

pub(crate) fn place_checks(
    facts: &RepoFacts,
    members: &[Member],
    grouped: bool,
    checks: bool,
    steps: Vec<Step>,
) -> Vec<Step> {
    let mut steps = with_checks(facts, members, grouped, checks, steps);
    gate_job(facts, &mut steps);
    steps
}

fn with_checks(
    facts: &RepoFacts,
    members: &[Member],
    grouped: bool,
    checks: bool,
    mut steps: Vec<Step>,
) -> Vec<Step> {
    if !crate::config_check_configurations::current(facts.helper.as_ref()) {
        return match members.first() {
            Some(member) if !grouped => checked(
                facts,
                &member.root,
                &member.lane,
                steps,
                Placement::Previous,
            ),
            _ => steps,
        };
    }
    if !checks {
        return steps;
    }
    for member in members {
        let placement = if member.tests(facts).is_empty() {
            Placement::Untested
        } else {
            Placement::Tested
        };
        steps = checked(facts, &member.root, &member.lane, steps, placement);
    }
    steps
}

#[derive(Clone, Copy)]
enum Placement {
    Previous,
    Tested,
    Untested,
}

fn checked(
    facts: &RepoFacts,
    root: &RustRoot,
    lane: &Lane,
    mut steps: Vec<Step>,
    placement: Placement,
) -> Vec<Step> {
    let current = !matches!(placement, Placement::Previous);
    let place = |step: &Step| match placement {
        Placement::Untested => builds_or_tests(step) && owns(step, root),
        Placement::Tested | Placement::Previous => {
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
    let target = if current {
        group_target(facts, root)
    } else {
        None
    };
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
                    phase: matches!(placement, Placement::Untested).then_some(SetupPhase::Build),
                },
                when,
                pr,
                current,
            )
        })
        .map(|check| Step {
            gate: lane.steps(),
            target_dir: target.clone(),
            setup_order: SetupOrder::AfterSetup,
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

pub(crate) fn gate_job(facts: &RepoFacts, steps: &mut [Step]) {
    let guards = lane_guard_ordered(facts);
    let (mut building, mut testing) = (false, false);
    for step in steps {
        match &step.kind {
            StepKind::TestSetup { before_build, .. } => {
                building |= *before_build;
                testing |= !*before_build;
            }
            StepKind::Cargo(_)
                if building || (testing && (tests(step) || (guards && lane_guard(step)))) =>
            {
                step.setup_order = SetupOrder::AfterSetup;
            }
            _ => {}
        }
    }
}

fn setup_step(
    setup: &TestSetup,
    when: velnor_model::When,
    pr: velnor_model::PrRun,
    offline_after_private_fetch: bool,
) -> Step {
    Step {
        directory: setup.root.clone(),
        when,
        pr,
        ..step(StepKind::TestSetup {
            name: setup.name.clone(),
            run: setup.run.clone(),
            before_build: setup.before_build(),
            offline_after_private_fetch,
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

fn lane_guard(step: &Step) -> bool {
    matches!(step.kind, StepKind::Cargo(CargoCommand::LaneGuard { .. }))
}

fn builds_or_tests(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Cargo(command) if command.builds()
        || matches!(command, CargoCommand::NextestRunArchive { .. }))
}
