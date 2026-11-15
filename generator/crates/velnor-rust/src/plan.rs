// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    Access, CargoCommand, Concurrency, FilterRef, FullRun, Gate, GateNeed, HelperCommand, Job,
    JobId, OnMain, PermissionScope, Permissions, PlanFrom, RepoFacts, RustMember, RustRoot, Shape,
    ShapeDecision, Step, StepKind, TargetKind, Triggers, When, WorkflowPlan,
};

use crate::config::CONFIG_PATH;
use crate::doc_steps::has_doctest_library;
use crate::error::{DetectError, Reason};
use crate::schedule::schedule;
use crate::select::root_slug;
use crate::shape::decide_shape;

const RUST_JOB: JobId = JobId::from_static("rust");
const LINT_JOB: JobId = JobId::from_static("rust-lint");
const TEST_JOB: JobId = JobId::from_static("rust-test");
const PLAN_JOB: JobId = JobId::from_static("rust-plan");
const VERDICT_JOB: JobId = JobId::from_static("required");
const CHECKOUT_DEPTH: u32 = 1;
const PLAN_DEPTH: u32 = 2;

struct Lane {
    slug: String,
    plan: Option<JobId>,
}

impl Lane {
    fn gate(&self, need: GateNeed) -> Gate {
        Gate {
            slug: self.slug.clone(),
            need,
            from: self.plan.clone().map_or(PlanFrom::ThisJob, PlanFrom::Job),
        }
    }

    fn steps(&self) -> Option<Gate> {
        self.plan.is_none().then(|| self.gate(GateNeed::Selected))
    }

    fn job(&self) -> Option<Gate> {
        self.plan.is_some().then(|| self.gate(GateNeed::Selected))
    }

    fn cargo(&self, command: &CargoCommand) -> Option<Gate> {
        match command {
            CargoCommand::Doctest => Some(self.gate(GateNeed::Doctests)),
            CargoCommand::Doc => Some(self.gate(GateNeed::Docs)),
            _ => self.steps(),
        }
    }
}

#[must_use]
pub fn plan(facts: &RepoFacts) -> WorkflowPlan {
    let suffixed = facts.rust_roots.len() > 1;
    let decisions: Vec<ShapeDecision> = facts
        .rust_roots
        .iter()
        .map(|root| decide_shape(facts.config.shape, facts.timings.get(&root.manifest)))
        .collect();
    let single = facts.rust_roots.len() == 1 && decisions[0].shape == Shape::OneJob;
    let mut jobs: Vec<Job> = facts
        .rust_roots
        .iter()
        .zip(decisions)
        .flat_map(|(root, decision)| {
            let lane = Lane {
                slug: root_slug(&root.manifest),
                plan: (!single).then_some(PLAN_JOB),
            };
            root_jobs(facts, root, decision, suffixed, &lane)
        })
        .collect();
    if !single {
        let used = used_outputs(&jobs);
        jobs.insert(0, plan_job(facts, used));
    }
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
            push_branch: Some(facts.config.default_branch.clone()),
            merge_group: true,
            schedule: schedule(facts),
            workflow_dispatch: true,
        },
        concurrency: Concurrency {
            cancel_in_progress: true,
        },
        permissions: Permissions(BTreeMap::from([(PermissionScope::Contents, Access::Read)])),
        jobs,
    }
}

fn used_outputs(lanes: &[Job]) -> Vec<(String, GateNeed)> {
    let used: BTreeSet<(String, GateNeed)> = lanes
        .iter()
        .flat_map(|job| {
            job.gate
                .iter()
                .chain(job.steps.iter().filter_map(|step| step.gate.as_ref()))
        })
        .flat_map(|gate| {
            [
                (gate.slug.clone(), GateNeed::Selected),
                (gate.slug.clone(), gate.need),
            ]
        })
        .collect();
    used.into_iter().collect()
}

fn plan_job(facts: &RepoFacts, outputs: Vec<(String, GateNeed)>) -> Job {
    let mut steps = head(facts, PLAN_DEPTH);
    steps.push(step(StepKind::Helper(HelperCommand::Plan)));
    Job {
        plan_outputs: outputs,
        steps,
        ..job(facts, PLAN_JOB)
    }
}

fn root_jobs(
    facts: &RepoFacts,
    root: &RustRoot,
    decision: ShapeDecision,
    suffixed: bool,
    lane: &Lane,
) -> Vec<Job> {
    let id = |base: JobId| {
        if suffixed {
            base.suffixed(&lane.slug)
        } else {
            base
        }
    };
    let cargo = |commands: Vec<CargoCommand>| {
        commands.into_iter().map(|command| Step {
            gate: lane.cargo(&command),
            kind: StepKind::Cargo(command),
            directory: root.manifest.parent(),
            when: When::NotCancelled,
        })
    };
    let lane_job = |id: JobId, steps: Vec<Step>| Job {
        needs: lane.plan.clone().into_iter().collect(),
        gate: lane.job(),
        steps,
        ..job(facts, id)
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
    let audit = audit_step(facts, lane);
    match decision.shape {
        Shape::OneJob => {
            let mut steps = setup(facts, root, true, lane);
            steps.extend(cargo(lint.into_iter().chain(test).collect()));
            steps.extend(audit);
            steps.extend(cargo(
                [CargoCommand::Doc].into_iter().chain(alone).collect(),
            ));
            vec![Job {
                comment,
                ..lane_job(id(RUST_JOB), steps)
            }]
        }
        Shape::LintAndTest | Shape::BuildThenGroups => {
            let mut lint_steps = setup(facts, root, true, lane);
            lint_steps.extend(cargo(
                lint.into_iter()
                    .chain([CargoCommand::Doc])
                    .chain(alone)
                    .collect(),
            ));
            if test.is_empty() {
                lint_steps.extend(audit.clone());
            }
            let mut jobs = vec![Job {
                comment,
                ..lane_job(id(LINT_JOB), lint_steps)
            }];
            if !test.is_empty() {
                let mut test_steps = setup(facts, root, false, lane);
                test_steps.extend(cargo(test));
                test_steps.extend(audit);
                jobs.push(lane_job(id(TEST_JOB), test_steps));
            }
            jobs
        }
    }
}

pub fn check_required_check(facts: &RepoFacts, plan: &WorkflowPlan) -> Result<(), DetectError> {
    let mut ids = BTreeSet::new();
    if let Some(job) = plan.jobs.iter().find(|job| !ids.insert(job.id.as_str())) {
        return Err(DetectError::JobIdTaken(job.id.to_string()));
    }
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

fn audit_step(facts: &RepoFacts, lane: &Lane) -> Option<Step> {
    let when = match (facts.config.on_main, &facts.config.full_run) {
        (OnMain::Everything, _) => When::ScheduledOrPush,
        (OnMain::Affected, FullRun::Off) => return None,
        (OnMain::Affected, _) => When::Scheduled,
    };
    Some(Step {
        when,
        gate: lane.steps(),
        ..step(StepKind::Helper(HelperCommand::InputsAudit))
    })
}

fn head(facts: &RepoFacts, depth: u32) -> Vec<Step> {
    let mut steps = vec![step(StepKind::Checkout { depth })];
    steps.extend(
        facts
            .helper
            .iter()
            .map(|release| step(StepKind::InstallHelper(release.clone()))),
    );
    steps
}

fn setup(facts: &RepoFacts, root: &RustRoot, lint: bool, lane: &Lane) -> Vec<Step> {
    let mut steps = head(
        facts,
        if lane.plan.is_none() {
            PLAN_DEPTH
        } else {
            CHECKOUT_DEPTH
        },
    );
    if lane.plan.is_none() {
        steps.push(step(StepKind::Helper(HelperCommand::Plan)));
    }
    let mut toolchain = Vec::new();
    if !facts.tools.tools.is_empty() {
        toolchain.push(step(StepKind::InstallTools));
    }
    let declared = facts.toolchain.as_ref();
    if declared.is_none_or(|toolchain| toolchain.path.is_none()) {
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
                toolchain.push(Step {
                    directory: root.manifest.parent(),
                    ..step(kind)
                });
            }
        }
    }
    toolchain.extend([
        step(StepKind::Helper(HelperCommand::Prepare)),
        step(StepKind::Helper(HelperCommand::Restored)),
    ]);
    steps.extend(toolchain.into_iter().map(|step| Step {
        gate: lane.steps(),
        ..step
    }));
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
        permissions: Permissions(BTreeMap::from([(PermissionScope::Contents, Access::Read)])),
        timeout_minutes: facts.config.timeout_minutes,
        gate: None,
        plan_outputs: Vec::new(),
        steps: Vec::new(),
    }
}

fn step(kind: StepKind) -> Step {
    Step {
        kind,
        directory: None,
        when: When::Success,
        gate: None,
    }
}
