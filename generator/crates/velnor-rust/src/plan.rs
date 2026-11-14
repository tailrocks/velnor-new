// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    Access, CargoCommand, Concurrency, FilterRef, Job, JobId, PermissionScope, Permissions,
    RepoFacts, RustMember, RustRoot, Shape, ShapeDecision, Step, StepKind, TargetKind, Triggers,
    When, WorkflowPlan,
};

use crate::config::CONFIG_PATH;
use crate::doc_steps::has_doctest_library;
use crate::error::{DetectError, Reason};
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
    let nextest = root.members.iter().any(RustMember::has_test_target);
    let mut test = Vec::new();
    if nextest {
        test.push(CargoCommand::NextestRun {
            filter: FilterRef::All,
        });
    }
    if has_doctest_library(root) {
        test.push(CargoCommand::Doctest);
    }
    let comment = Some(decision.reason);
    match decision.shape {
        Shape::OneJob => {
            let mut steps = setup(facts, root, true);
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
            let mut lint_steps = setup(facts, root, true);
            lint_steps.extend(cargo(
                lint.into_iter()
                    .chain([CargoCommand::Doc])
                    .chain(alone)
                    .collect(),
            ));
            let mut jobs = vec![Job {
                comment,
                steps: lint_steps,
                ..job(facts, id(LINT_JOB))
            }];
            if !test.is_empty() {
                let mut test_steps = setup(facts, root, false);
                test_steps.extend(cargo(test));
                jobs.push(Job {
                    steps: test_steps,
                    ..job(facts, id(TEST_JOB))
                });
            }
            jobs
        }
    }
}

pub fn check_required_check(facts: &RepoFacts, plan: &WorkflowPlan) -> Result<(), DetectError> {
    let name = &facts.config.required_check;
    if plan
        .jobs
        .iter()
        .any(|job| job.name.is_none() && job.id.as_str() == name)
    {
        return Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::InvalidValue {
                key: "required_check",
                value: name.clone(),
            },
        ));
    }
    Ok(())
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

fn setup(facts: &RepoFacts, root: &RustRoot, lint: bool) -> Vec<Step> {
    let mut steps = vec![step(StepKind::Checkout { depth: 1 })];
    steps.extend(
        facts
            .helper
            .iter()
            .map(|release| step(StepKind::FetchHelper(release.clone()))),
    );
    if !facts.tools.tools.is_empty() {
        steps.push(step(StepKind::InstallTools));
    }
    let declared = facts.toolchain.as_ref();
    if declared.is_some_and(|toolchain| toolchain.path.is_some()) {
        return steps;
    }
    let mut components: Vec<String> = declared
        .map(|toolchain| toolchain.components.clone())
        .unwrap_or_default();
    if lint {
        components.extend(["rustfmt".to_owned(), "clippy".to_owned()]);
    }
    components.sort();
    components.dedup();
    let targets = declared
        .map(|toolchain| toolchain.targets.clone())
        .unwrap_or_default();
    for (kind, names) in [
        (StepKind::RustComponents(components.clone()), &components),
        (StepKind::RustTargets(targets.clone()), &targets),
    ] {
        if !names.is_empty() {
            steps.push(Step {
                directory: root.manifest.parent(),
                ..step(kind)
            });
        }
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
