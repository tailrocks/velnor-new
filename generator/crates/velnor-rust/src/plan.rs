// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    Access, Concurrency, GateNeed, HelperCommand, Job, JobId, PermissionScope, Permissions,
    RepoFacts, RustRoot, Shape, ShapeDecision, Step, StepKind, Triggers, When, WorkflowPlan,
};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};
use crate::root_jobs::{Lane, root_jobs};
use crate::select::root_slug;
use crate::shape::decide_shape;

const PLAN_JOB: JobId = JobId::from_static("rust-plan");
const VERDICT_JOB: JobId = JobId::from_static("required");

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
    let mut steps = head(facts);
    steps.push(step(StepKind::Helper(HelperCommand::Plan)));
    Job {
        plan_outputs: outputs,
        steps,
        permissions: Permissions(BTreeMap::from([
            (PermissionScope::Contents, Access::Read),
            (PermissionScope::PullRequests, Access::Read),
        ])),
        ..job(facts, PLAN_JOB)
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

pub(crate) fn head(facts: &RepoFacts) -> Vec<Step> {
    let mut steps = vec![step(StepKind::Checkout { depth: 1 })];
    steps.extend(
        facts
            .helper
            .iter()
            .map(|release| step(StepKind::InstallHelper(release.clone()))),
    );
    steps
}

pub(crate) fn setup(facts: &RepoFacts, root: &RustRoot, lint: bool, lane: &Lane) -> Vec<Step> {
    let mut steps = head(facts);
    if lane.plan.is_none() {
        steps.push(step(StepKind::Helper(HelperCommand::Plan)));
    }
    let mut toolchain = Vec::new();
    if !facts.tools.tools.is_empty() {
        toolchain.push(step(StepKind::InstallTools { only: None }));
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

pub(crate) fn job(facts: &RepoFacts, id: JobId) -> Job {
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

pub(crate) fn step(kind: StepKind) -> Step {
    Step {
        kind,
        directory: None,
        when: When::Success,
        gate: None,
    }
}
