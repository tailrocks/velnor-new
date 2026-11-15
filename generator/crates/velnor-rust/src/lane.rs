// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    Access, CargoCommand, Gate, GateNeed, HelperCommand, Job, JobId, PermissionScope, Permissions,
    PlanFrom, PlanRef, RepoFacts, Step, StepKind, When,
};

use crate::plan::{head, job, step};

pub(crate) const PLAN_JOB: JobId = JobId::from_static("rust-plan");
const POLICY_JOB: JobId = JobId::from_static("rust-policy");

pub(crate) struct Lane {
    pub(crate) slug: String,
    pub(crate) plan: Option<JobId>,
}

impl Lane {
    pub(crate) fn root(slug: String, single: bool) -> Self {
        Self {
            slug,
            plan: (!single).then_some(PLAN_JOB),
        }
    }

    pub(crate) fn policy(single: bool) -> Self {
        Self::root(String::new(), single)
    }

    fn from(&self) -> PlanFrom {
        self.plan.clone().map_or(PlanFrom::ThisJob, PlanFrom::Job)
    }

    fn gate(&self, need: GateNeed) -> Gate {
        Gate {
            slug: self.slug.clone(),
            need,
            from: self.from(),
        }
    }

    pub(crate) fn steps(&self) -> Option<Gate> {
        self.plan.is_none().then(|| self.gate(GateNeed::Selected))
    }

    pub(crate) fn job(&self) -> Option<Gate> {
        self.plan.is_some().then(|| self.gate(GateNeed::Selected))
    }

    pub(crate) fn cargo(
        &self,
        command: CargoCommand,
        directory: Option<velnor_model::RepoPath>,
    ) -> Step {
        let gate = match command {
            CargoCommand::Doctest => Some(self.gate(GateNeed::Doctests)),
            CargoCommand::Doc => Some(self.gate(GateNeed::Docs)),
            CargoCommand::Deny => Some(self.gate(GateNeed::Deny)),
            CargoCommand::Machete => Some(self.gate(GateNeed::Machete)),
            _ => self.steps(),
        };
        let reads = (!command.plan_values().is_empty()).then(|| PlanRef {
            slug: self.slug.clone(),
            from: self.from(),
        });
        Step {
            gate,
            reads,
            kind: StepKind::Cargo(command),
            directory,
            when: When::NotCancelled,
        }
    }
}

pub(crate) fn used_outputs(jobs: &[Job]) -> Vec<(String, GateNeed)> {
    let used: BTreeSet<(String, GateNeed)> = jobs
        .iter()
        .flat_map(|job| {
            let gates = job
                .gate
                .iter()
                .chain(job.steps.iter().filter_map(|step| step.gate.as_ref()))
                .flat_map(Gate::outputs);
            let reads = job.steps.iter().flat_map(|step| {
                let slug = step.reads.iter().map(|plan| plan.slug.clone());
                let needs: &[GateNeed] = match &step.kind {
                    StepKind::Cargo(command) => command.plan_values(),
                    _ => &[],
                };
                slug.flat_map(move |slug| needs.iter().map(move |need| (slug.clone(), *need)))
            });
            gates.chain(reads)
        })
        .collect();
    used.into_iter().collect()
}

pub(crate) fn plan_job(facts: &RepoFacts, outputs: Vec<(String, GateNeed)>) -> Job {
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

pub(crate) fn policy_job(facts: &RepoFacts, lane: &Lane) -> Job {
    let mut steps = head(facts);
    if lane.plan.is_none() {
        steps.push(step(StepKind::Helper(HelperCommand::Plan)));
    }
    let tools = [
        (facts.policy.deny, CargoCommand::Deny),
        (facts.policy.machete, CargoCommand::Machete),
    ];
    steps.extend(
        tools
            .into_iter()
            .filter(|(wanted, _)| *wanted)
            .map(|(_, command)| lane.cargo(command, None)),
    );
    Job {
        needs: lane.plan.clone().into_iter().collect(),
        gate: lane.plan.is_some().then(|| lane.gate(GateNeed::Policy)),
        steps,
        ..job(facts, POLICY_JOB)
    }
}
