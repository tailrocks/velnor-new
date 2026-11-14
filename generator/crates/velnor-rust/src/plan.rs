// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    Access, CargoCommand, Concurrency, FilterRef, Job, JobId, PermissionScope, Permissions,
    RepoFacts, RustRoot, Shape, ShapeDecision, Step, StepKind, TargetKind, Triggers, When,
    WorkflowPlan,
};

use crate::doc_steps::has_doctest_library;
use crate::mise::{nextest_tool, rust_tool};
use crate::select::root_slug;
use crate::shape::decide_shape;

const RUST_JOB: JobId = JobId::from_static("rust");
const LINT_JOB: JobId = JobId::from_static("rust-lint");
const TEST_JOB: JobId = JobId::from_static("rust-test");
const VERDICT_JOB: JobId = JobId::from_static("required");

#[must_use]
pub fn plan(facts: &RepoFacts) -> WorkflowPlan {
    let suffixed = facts.rust_roots.len() > 1;
    let mut jobs: Vec<Job> = facts
        .rust_roots
        .iter()
        .flat_map(|root| {
            let decision = decide_shape(facts.config.shape, facts.timings.get(&root.manifest));
            root_jobs(facts, root, decision, suffixed)
        })
        .collect();
    jobs.push(Job {
        name: Some(facts.config.required_check.clone()),
        needs: jobs.iter().map(|job| job.id.clone()).collect(),
        when: When::NotCancelled,
        steps: vec![step(StepKind::FailWhenANeedFailed)],
        ..job(facts, VERDICT_JOB)
    });
    WorkflowPlan {
        name: "Rust CI".to_owned(),
        triggers: Triggers {
            pull_request: true,
            workflow_dispatch: true,
        },
        concurrency: Concurrency {
            cancel_in_progress: true,
        },
        permissions: Permissions(BTreeMap::from([(PermissionScope::Contents, Access::Read)])),
        jobs,
    }
}

fn root_jobs(
    facts: &RepoFacts,
    root: &RustRoot,
    decision: ShapeDecision,
    suffixed: bool,
) -> Vec<Job> {
    let id = |base: JobId| {
        if suffixed {
            base.suffixed(&root_slug(&root.manifest))
        } else {
            base
        }
    };
    let cargo = |commands: Vec<CargoCommand>| {
        commands.into_iter().map(|command| Step {
            kind: StepKind::Cargo(command),
            directory: root.manifest.parent(),
            when: When::NotCancelled,
        })
    };
    let alone = check_alone(root);
    let lint = [
        CargoCommand::FmtCheck,
        CargoCommand::Clippy { extra: Vec::new() },
    ];
    let mut test = vec![CargoCommand::NextestRun {
        filter: FilterRef::All,
    }];
    if has_doctest_library(root) {
        test.push(CargoCommand::Doctest);
    }
    let comment = Some(decision.reason);
    match decision.shape {
        Shape::OneJob => {
            let mut steps = setup(facts, true, true);
            steps.extend(cargo(
                lint.into_iter()
                    .chain(test)
                    .chain([CargoCommand::Doc])
                    .chain(alone)
                    .collect(),
            ));
            vec![Job {
                comment,
                steps,
                ..job(facts, id(RUST_JOB))
            }]
        }
        Shape::LintAndTest | Shape::BuildThenGroups => {
            let mut lint_steps = setup(facts, true, false);
            lint_steps.extend(cargo(
                lint.into_iter()
                    .chain([CargoCommand::Doc])
                    .chain(alone)
                    .collect(),
            ));
            let mut test_steps = setup(facts, false, true);
            test_steps.extend(cargo(test));
            vec![
                Job {
                    comment,
                    steps: lint_steps,
                    ..job(facts, id(LINT_JOB))
                },
                Job {
                    steps: test_steps,
                    ..job(facts, id(TEST_JOB))
                },
            ]
        }
    }
}

fn check_alone(root: &RustRoot) -> Vec<CargoCommand> {
    root.members
        .iter()
        .filter(|member| {
            member.targets.iter().any(|target| {
                !target.requires_features
                    && target.kinds.iter().any(|kind| {
                        matches!(
                            kind,
                            TargetKind::Lib | TargetKind::ProcMacro | TargetKind::Bin
                        )
                    })
            })
        })
        .map(|member| CargoCommand::CheckAlone {
            package: member.name.clone(),
        })
        .collect()
}

fn setup(facts: &RepoFacts, lint: bool, tests: bool) -> Vec<Step> {
    let mut tools: Vec<String> = rust_tool(&facts.tools)
        .map(str::to_owned)
        .into_iter()
        .collect();
    if tests {
        tools.extend(nextest_tool(&facts.tools).map(str::to_owned));
    }
    let mut steps = vec![step(StepKind::Checkout { depth: 1 })];
    steps.extend(
        facts
            .helper
            .iter()
            .map(|release| step(StepKind::FetchHelper(release.clone()))),
    );
    if !tools.is_empty() {
        steps.push(step(StepKind::InstallTools(tools)));
    }
    if lint {
        steps.push(step(StepKind::LintComponents));
    }
    steps
}

fn job(facts: &RepoFacts, id: JobId) -> Job {
    Job {
        id,
        name: None,
        comment: None,
        needs: Vec::new(),
        when: When::Success,
        runs_on: facts.config.runner.clone(),
        timeout_minutes: facts.config.timeout_minutes,
        steps: Vec::new(),
    }
}

fn step(kind: StepKind) -> Step {
    Step {
        kind,
        directory: None,
        when: When::Success,
    }
}
