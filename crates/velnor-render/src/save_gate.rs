// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CargoCommand, Gate, HelperCommand, Job, PrRun, Step, StepKind, When};

use crate::gate::Setup;
use crate::rules::RenderedStep;
use crate::save_condition::{Audited, Built, Compiled, audit_passed, audit_runs, builds_finished};
use crate::tests::Tests;

const BUILD_ID: &str = "build";
const AUDIT_ID: &str = "audit";
const COMPILE_ID: &str = "compile";
const SAVEABLE_ID: &str = "saveable";

#[derive(Clone, Debug, Default)]
pub(crate) struct SaveGate {
    ids: Vec<Option<String>>,
    required: Vec<String>,
    audit_runs: Vec<Option<String>>,
    saveable: Option<Saveable>,
}

#[derive(Clone, Debug)]
struct Saveable {
    at: usize,
    terms: Vec<String>,
    gate: Option<Gate>,
    setup: Setup,
}

fn is_save(step: &Step) -> bool {
    matches!(
        step.when,
        When::SaveBase | When::SaveBaseDecided | When::SaveSources | When::FetchSources
    )
}

fn is_audit(step: &Step) -> bool {
    matches!(
        step.kind,
        StepKind::Helper(HelperCommand::InputsAudit) | StepKind::InputsAuditRoot(_)
    )
}

fn is_build(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Cargo(command) if command.is_target_build())
}

fn is_compile_check(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Cargo(command) if command.is_compile_check())
}

fn is_test_run(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Cargo(CargoCommand::NextestRun { .. }))
}

impl SaveGate {
    pub(crate) fn of(job: &Job) -> Self {
        let mut gate = Self {
            ids: vec![None; job.steps.len()],
            audit_runs: vec![None; job.steps.len()],
            ..Self::default()
        };
        let Some(first) = job.steps.iter().position(is_save) else {
            return gate;
        };
        let saved_here = job.steps[first].gate.as_ref();
        let legacy = !job.records_build
            || job
                .steps
                .iter()
                .any(|step| matches!(&step.kind, StepKind::Cargo(CargoCommand::NextestBuild)));
        let tests = Tests::of(job);
        let restores = crate::base_steps::job_setup(job).0;
        let (mut compiled, mut audits) = (Vec::<Compiled>::new(), Vec::new());
        let mut checks = Vec::<String>::new();
        let mut replanned = false;
        for (index, step) in job.steps.iter().enumerate().take(first) {
            let setup = Setup::of(restores, replanned);
            replanned |= step.kind == StepKind::Helper(HelperCommand::PlanFull);
            if !legacy && is_test_run(step) {
                let test = tests.id(step.reads.as_ref().map(|plan| &plan.slug));
                compiled.push(Compiled {
                    built: Built::Recorded(test),
                    gate: step.gate.clone(),
                    setup,
                });
            } else if is_build(step) {
                let id = format!("{BUILD_ID}_{}", compiled.len() + 1);
                gate.ids[index] = Some(id.clone());
                compiled.push(Compiled {
                    built: Built::Step(id),
                    gate: step.gate.clone(),
                    setup,
                });
            } else if is_compile_check(step) && (!legacy || !compiled.is_empty()) {
                let id = format!("{COMPILE_ID}_{}", checks.len() + 1);
                gate.ids[index] = Some(id.clone());
                checks.push(id);
            } else if is_audit(step) {
                let id = format!("{AUDIT_ID}_{}", audits.len() + 1);
                gate.ids[index] = Some(id.clone());
                if !compiled.is_empty() {
                    let before = audit_runs(&compiled, saved_here);
                    gate.audit_runs[index] = Some(format!("${{{{ {} }}}}", before.join(" && ")));
                }
                audits.push(Audited {
                    id,
                    when: step.when,
                    setup,
                });
            }
        }
        let mut terms = Vec::new();
        if compiled.is_empty() {
            let ids = std::mem::take(&mut gate.ids);
            gate.ids = ids
                .into_iter()
                .map(|id| id.filter(|id| checks.contains(id)))
                .collect();
            gate.audit_runs.fill(None);
            if !checks.is_empty() {
                terms.push("success()".to_owned());
            }
        } else {
            terms = builds_finished(&compiled, saved_here);
            terms.extend(audits.iter().map(audit_passed));
        }
        if !terms.is_empty() {
            terms.extend(
                checks
                    .iter()
                    .map(|id| format!("steps.{id}.outcome != 'failure'")),
            );
            let setup = Setup::of(restores, replanned);
            if checks.is_empty() || setup == Setup::Before {
                gate.required = terms;
            } else {
                gate.required = vec![format!("steps.{SAVEABLE_ID}.outcome == 'success'")];
                gate.saveable = Some(Saveable {
                    at: first,
                    terms,
                    gate: saved_here.cloned(),
                    setup,
                });
            }
        }
        gate
    }

    pub(crate) fn id(&self, index: usize) -> Option<&str> {
        self.ids.get(index)?.as_deref()
    }

    pub(crate) fn saveable(&self, index: usize) -> Option<RenderedStep> {
        let saveable = self.saveable.as_ref().filter(|found| found.at == index)?;
        Some(RenderedStep {
            id: Some(SAVEABLE_ID.to_owned()),
            name: "Record that the build can be saved".to_owned(),
            condition: crate::gate::condition(
                When::SaveBase,
                saveable.gate.as_ref(),
                saveable.setup,
                PrRun::Always,
                &[],
                &saveable.terms,
            ),
            run: Some("echo 'the build can be saved'".to_owned()),
            ..RenderedStep::default()
        })
    }

    pub(crate) fn required(&self) -> &[String] {
        &self.required
    }

    pub(crate) fn audit_runs(&self, index: usize) -> Option<&str> {
        self.audit_runs.get(index)?.as_deref()
    }
}

pub(crate) fn settled(saved_after: &[String]) -> Vec<String> {
    if saved_after.is_empty() {
        vec!["success()".to_owned()]
    } else {
        saved_after.to_vec()
    }
}
