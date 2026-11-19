// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{
    CargoCommand, Gate, GateNeed, HelperCommand, Job, JobId, PackageName, PlanFrom, PlanRef, PrCi,
    PrRun, RepoFacts, RootSlug, RustRoot, Step, StepKind, When,
};

use crate::plan::{CHECKOUT_DEPTH, PLAN_DEPTH, RUST_JOB, head, job, step};
use crate::plan_tools::{mirror_step, tools_install, tools_restore, tools_save};

pub(crate) const PLAN_JOB: JobId = JobId::from_static("rust-plan");
const POLICY_JOB: JobId = JobId::from_static("policy");

pub(crate) struct Lane {
    pub(crate) slug: RootSlug,
    pub(crate) plan: Option<JobId>,
    pub(crate) deltas: bool,
}

impl Lane {
    pub(crate) fn root(slug: RootSlug, single: bool, deltas: bool) -> Self {
        Self {
            slug,
            plan: (!single).then_some(PLAN_JOB),
            deltas,
        }
    }

    pub(crate) fn policy(single: bool, deltas: bool) -> Self {
        Self {
            slug: RootSlug::from_static("policy"),
            plan: Some(if single { RUST_JOB } else { PLAN_JOB }),
            deltas,
        }
    }

    fn from(&self) -> PlanFrom {
        self.plan.clone().map_or(PlanFrom::ThisJob, PlanFrom::Job)
    }

    fn gate(&self, need: GateNeed) -> Gate {
        Gate {
            slug: self.slug.clone(),
            need,
            from: self.from(),
            packages: Vec::new(),
            unselected: false,
            group: None,
        }
    }

    pub(crate) fn group_job(&self, group: JobId) -> Option<Gate> {
        self.plan.is_some().then(|| Gate {
            group: Some(group),
            ..self.gate(GateNeed::Group)
        })
    }

    fn package_gate(&self, packages: Vec<PackageName>) -> Gate {
        Gate {
            packages,
            ..self.gate(GateNeed::PackageArgs)
        }
    }

    /// The notice that stands in for a feature lane the plan did not select.
    pub(crate) fn skipped_lane(
        &self,
        features: &[String],
        packages: &[PackageName],
        root: &RustRoot,
    ) -> Step {
        Step {
            gate: Some(Gate {
                unselected: true,
                ..self.package_gate(packages.to_vec())
            }),
            directory: root.manifest.parent(),
            when: When::NotCancelled,
            ..step(StepKind::TestSetup {
                name: format!("Skip test lane (features: {})", features.join(",")),
                run: "echo 'skipped: no selected packages'".to_owned(),
            })
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
            CargoCommand::Doc | CargoCommand::DocFeatures { .. } => Some(self.gate(GateNeed::Docs)),
            CargoCommand::Deny { .. } => Some(self.gate(GateNeed::Deny)),
            CargoCommand::Machete => Some(self.gate(GateNeed::Machete)),
            CargoCommand::CheckAlone { ref package }
            | CargoCommand::TestTarget { ref package, .. } => {
                Some(self.package_gate(vec![package.clone()]))
            }
            CargoCommand::NextestFeatures { ref packages, .. }
            | CargoCommand::LaneGuard { ref packages, .. }
                if !packages.is_empty() =>
            {
                Some(self.package_gate(packages.clone()))
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

pub(crate) fn used_group_outputs(jobs: &[Job]) -> Vec<(RootSlug, JobId)> {
    let used: BTreeSet<(RootSlug, JobId)> = jobs
        .iter()
        .filter_map(|job| job.gate.as_ref())
        .filter_map(|gate| Some((gate.slug.clone(), gate.group.clone()?)))
        .collect();
    used.into_iter().collect()
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

pub(crate) fn expose_read_outputs(jobs: &mut [Job], source: &JobId) {
    let read: BTreeSet<(RootSlug, GateNeed)> = jobs
        .iter()
        .filter(|job| &job.id != source)
        .flat_map(|job| {
            job.gate
                .iter()
                .chain(job.steps.iter().filter_map(|step| step.gate.as_ref()))
        })
        .filter(|gate| matches!(&gate.from, PlanFrom::Job(job) if job == source))
        .flat_map(Gate::outputs)
        .collect();
    for job in jobs.iter_mut().filter(|job| &job.id == source) {
        job.plan_outputs.extend(read.iter().cloned());
        job.plan_outputs.sort();
        job.plan_outputs.dedup();
    }
}

pub(crate) fn plan_job(
    facts: &RepoFacts,
    outputs: Vec<(RootSlug, GateNeed)>,
    group_outputs: Vec<(RootSlug, JobId)>,
) -> Job {
    let mut steps = head(facts, PLAN_DEPTH, None);
    steps.push(step(StepKind::Helper(HelperCommand::Plan)));
    Job {
        plan_outputs: outputs,
        group_outputs,
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
        None,
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
                    config: facts.deny_configs.get(&root.manifest).cloned(),
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
