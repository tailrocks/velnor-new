// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{
    CargoCommand, Gate, GateNeed, HelperCommand, Job, JobId, PackageName, PlanFrom, PlanRef, PrCi,
    PrRun, RepoFacts, RootSlug, RustRoot, Step, StepKind, When,
};

use crate::plan::{
    CHECKOUT_DEPTH, PLAN_DEPTH, head, job, mirror_step, step, tools_install, tools_restore,
    tools_save,
};

pub(crate) const PLAN_JOB: JobId = JobId::from_static("rust-plan");
const POLICY_JOB: JobId = JobId::from_static("policy");

pub(crate) struct Lane {
    pub(crate) slug: RootSlug,
    pub(crate) plan: Option<JobId>,
}

impl Lane {
    pub(crate) fn root(slug: RootSlug, single: bool) -> Self {
        Self {
            slug,
            plan: (!single).then_some(PLAN_JOB),
        }
    }

    pub(crate) fn policy(single: bool) -> Self {
        Self::root(RootSlug::from_static("policy"), single)
    }

    fn from(&self) -> PlanFrom {
        self.plan.clone().map_or(PlanFrom::ThisJob, PlanFrom::Job)
    }

    fn gate(&self, need: GateNeed) -> Gate {
        Gate {
            slug: self.slug.clone(),
            need,
            from: self.from(),
            package: None,
        }
    }

    fn package_gate(&self, package: PackageName) -> Gate {
        Gate {
            package: Some(package),
            ..self.gate(GateNeed::PackageArgs)
        }
    }

    pub(crate) fn steps(&self) -> Option<Gate> {
        self.plan.is_none().then(|| self.gate(GateNeed::Selected))
    }

    pub(crate) fn job(&self) -> Option<Gate> {
        self.plan.is_some().then(|| self.gate(GateNeed::Selected))
    }

    pub(crate) fn cargo(&self, command: CargoCommand, root: Option<&RustRoot>) -> Step {
        let gate = match command {
            CargoCommand::Doctest => Some(self.gate(GateNeed::Doctests)),
            CargoCommand::Doc => Some(self.gate(GateNeed::Docs)),
            CargoCommand::Deny { .. } => Some(self.gate(GateNeed::Deny)),
            CargoCommand::Machete => Some(self.gate(GateNeed::Machete)),
            CargoCommand::CheckAlone { ref package }
            | CargoCommand::TestTarget { ref package, .. } => {
                Some(self.package_gate(package.clone()))
            }
            _ => self.steps(),
        };
        let reads = (!command.plan_values().is_empty()).then(|| PlanRef {
            slug: self.slug.clone(),
            from: self.from(),
        });
        Step {
            gate,
            reads,
            directory: root.and_then(|root| root.manifest.parent()),
            target_dir: root
                .filter(|_| command.builds())
                .map(|root| root.target_dir.clone()),
            kind: StepKind::Cargo(command),
            when: When::NotCancelled,
            skip_on_draft: false,
            skip_after_merge_group: None,
            pr: PrRun::Always,
        }
    }
}

pub(crate) fn used_outputs(jobs: &[Job]) -> Vec<(RootSlug, GateNeed)> {
    let used: BTreeSet<(RootSlug, GateNeed)> = jobs
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

pub(crate) fn plan_job(facts: &RepoFacts, outputs: Vec<(RootSlug, GateNeed)>) -> Job {
    let mut steps = head(facts, PLAN_DEPTH);
    steps.push(step(StepKind::Helper(HelperCommand::Plan)));
    Job {
        plan_outputs: outputs,
        pr: crate::plan::quick(facts),
        steps,
        ..job(facts, PLAN_JOB)
    }
}

pub(crate) fn policy_job(facts: &RepoFacts, lane: &Lane) -> Job {
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
    let policy_gate = lane.plan.is_none().then(|| lane.gate(GateNeed::Policy));
    steps.extend(mirror_step(facts).map(|step| Step {
        gate: policy_gate.clone(),
        ..step
    }));
    if !facts.tools.tools.is_empty() {
        steps.extend(
            tools_restore(facts)
                .into_iter()
                .chain(tools_install(facts))
                .chain(mirror_step(facts))
                .chain(tools_save(facts))
                .map(|step| Step {
                    gate: policy_gate.clone(),
                    ..step
                }),
        );
    }
    if facts.policy.deny {
        steps.extend(facts.rust_roots.iter().map(|root| {
            lane.cargo(
                CargoCommand::Deny {
                    manifest: root.manifest.clone(),
                },
                None,
            )
        }));
    }
    if facts.policy.machete {
        steps.push(lane.cargo(CargoCommand::Machete, None));
    }
    Job {
        needs: lane.plan.clone().into_iter().collect(),
        gate: lane.plan.is_some().then(|| lane.gate(GateNeed::Policy)),
        pr: if facts.config.pr_ci == PrCi::Label {
            PrRun::Quick
        } else {
            crate::plan::full(facts)
        },
        steps,
        ..job(facts, POLICY_JOB)
    }
}
