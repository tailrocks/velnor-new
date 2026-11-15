// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    Access, CargoCommand, Concurrency, CustomHarness, FilterRef, Gate, GateNeed, HelperCommand,
    Job, JobId, PackageName, PermissionScope, Permissions, PlanFrom, PullRequestType, RepoFacts,
    RustRoot, Shape, ShapeDecision, Step, StepKind, TargetKind, Triggers, When, WorkflowPlan,
};

use crate::config::CONFIG_PATH;
use crate::doc_steps::has_doctest_library;
use crate::error::{DetectError, Reason};
use crate::harness::{runs_in_direct_step, selector_id, uses_nextest};
use crate::select::root_slug;
use crate::shape::decide_shape;

pub(crate) const RUST_JOB: JobId = JobId::from_static("rust");
pub(crate) const LINT_JOB: JobId = JobId::from_static("rust-lint");
pub(crate) const TEST_JOB: JobId = JobId::from_static("rust-test");
const POLICY_JOB: JobId = JobId::from_static("policy");
const PLAN_JOB: JobId = JobId::from_static("rust-plan");
const VERDICT_JOB: JobId = JobId::from_static("required");

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
    if facts.policy.any() {
        jobs.push(policy_job(facts));
    }
    let draft_guard = facts
        .config
        .skip_draft_pull_requests
        .then(|| step(StepKind::FailOnDraftRun));
    if !single {
        let used = used_outputs(&jobs);
        jobs.insert(0, plan_job(facts, used));
    }
    jobs.push(Job {
        name: Some(facts.config.required_check.clone()),
        skip_on_draft: false,
        needs: jobs.iter().map(|job| job.id.clone()).collect(),
        when: When::NotCancelled,
        steps: draft_guard
            .into_iter()
            .chain([step(StepKind::FailWhenANeedFailed)])
            .collect(),
        ..job(facts, VERDICT_JOB)
    });
    WorkflowPlan {
        name: "Rust CI".to_owned(),
        triggers: Triggers {
            pull_request: PullRequestType::ALL
                .into_iter()
                .chain(
                    facts
                        .config
                        .skip_draft_pull_requests
                        .then_some(PullRequestType::ConvertedToDraft),
                )
                .collect(),
            push_branch: Some(facts.config.default_branch.clone()),
            merge_group: true,
            workflow_dispatch: true,
        },
        concurrency: Concurrency::CancelOlderPullRequestRuns,
        permissions: Permissions(BTreeMap::from([
            (PermissionScope::Contents, Access::Read),
            (PermissionScope::PullRequests, Access::Read),
        ])),
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
    let mut steps = head(facts);
    steps.push(step(StepKind::Helper(HelperCommand::Plan)));
    Job {
        plan_outputs: outputs,
        skip_on_draft: false,
        steps,
        permissions: Permissions(BTreeMap::from([
            (PermissionScope::Contents, Access::Read),
            (PermissionScope::PullRequests, Access::Read),
        ])),
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
    let test = test_commands(facts, root);
    let comment = Some(decision.reason);
    match decision.shape {
        Shape::OneJob => {
            let mut steps = setup(facts, root, true, lane);
            steps.extend(cargo(
                lint.into_iter()
                    .chain(test)
                    .chain([CargoCommand::Doc])
                    .chain(alone)
                    .collect(),
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
            let mut jobs = vec![Job {
                comment,
                skip_on_draft: false,
                ..lane_job(id(LINT_JOB), lint_steps)
            }];
            if !test.is_empty() {
                let mut test_steps = setup(facts, root, false, lane);
                test_steps.extend(cargo(test));
                jobs.push(lane_job(id(TEST_JOB), test_steps));
            }
            jobs
        }
    }
}

fn test_commands(facts: &RepoFacts, root: &RustRoot) -> Vec<CargoCommand> {
    let mut test = Vec::new();
    if root
        .members
        .iter()
        .any(|member| uses_nextest(&facts.config, member))
    {
        test.push(CargoCommand::NextestRun {
            filter: nextest_filter(facts, root),
        });
    }
    let mut seen = BTreeSet::new();
    test.extend(root.members.iter().flat_map(|member| {
        member
            .custom_harnesses
            .iter()
            .filter(|harness| runs_in_direct_step(&facts.config, member, harness))
            .filter(|harness| seen.insert(selector_id(harness)))
            .map(|harness| CargoCommand::TestTarget {
                package: member.name.clone(),
                target: harness.name.clone(),
                kind: harness.kind,
            })
            .collect::<Vec<_>>()
    }));
    if has_doctest_library(root) {
        test.push(CargoCommand::Doctest);
    }
    test
}

fn nextest_filter(facts: &RepoFacts, root: &RustRoot) -> FilterRef {
    let direct: Vec<String> = root
        .members
        .iter()
        .flat_map(|member| {
            member
                .custom_harnesses
                .iter()
                .filter(|harness| runs_in_direct_step(&facts.config, member, harness))
                .map(|harness| binary_id(&member.name, harness))
        })
        .collect();
    if direct.is_empty() {
        FilterRef::All
    } else {
        FilterRef::AllExcept(direct)
    }
}

fn binary_id(package: &PackageName, harness: &CustomHarness) -> String {
    let name = &harness.name;
    match harness.kind {
        TargetKind::Lib | TargetKind::ProcMacro => package.to_string(),
        TargetKind::Bin => format!("{package}::bin/{name}"),
        TargetKind::Example => format!("{package}::example/{name}"),
        TargetKind::Bench => format!("{package}::bench/{name}"),
        TargetKind::Test | TargetKind::BuildScript => format!("{package}::{name}"),
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

fn policy_job(facts: &RepoFacts) -> Job {
    let commands = [
        (facts.policy.deny, CargoCommand::Deny),
        (facts.policy.machete, CargoCommand::Machete),
    ];
    let mut steps = vec![step(StepKind::Checkout { depth: 1 })];
    steps.extend(tools_steps(facts));
    steps.extend(
        commands
            .into_iter()
            .filter(|(wanted, _)| *wanted)
            .map(|(_, command)| Step {
                when: When::NotCancelled,
                ..step(StepKind::Cargo(command))
            }),
    );
    Job {
        steps,
        ..job(facts, POLICY_JOB)
    }
}

fn head(facts: &RepoFacts) -> Vec<Step> {
    let mut steps = vec![step(StepKind::Checkout { depth: 1 })];
    steps.extend(
        facts
            .helper
            .iter()
            .map(|release| step(StepKind::InstallHelper(release.clone()))),
    );
    steps
}

fn tools_steps(facts: &RepoFacts) -> Option<Step> {
    (!facts.tools.tools.is_empty()).then(|| step(StepKind::InstallTools))
}

fn setup(facts: &RepoFacts, root: &RustRoot, lint: bool, lane: &Lane) -> Vec<Step> {
    let mut steps = head(facts);
    if lane.plan.is_none() {
        steps.push(step(StepKind::Helper(HelperCommand::Plan)));
    }
    let mut toolchain: Vec<Step> = tools_steps(facts).into_iter().collect();
    let declared = facts.toolchain.as_ref();
    if declared.is_none_or(|toolchain| toolchain.path.is_none()) {
        let mut components: Vec<String> = declared
            .map(|toolchain| toolchain.components.clone())
            .unwrap_or_default();
        if declared.is_some_and(|toolchain| toolchain.profile.as_deref() == Some("default")) {
            components.push("rust-docs".to_owned());
        }
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
        skip_on_draft: facts.config.skip_draft_pull_requests,
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
