// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! What the saves of the target and sources caches wait for.
//!
//! A save does not wait for the whole job to succeed: a failing test must not cost
//! the next run its warm target. It waits for the steps that compile the build it
//! captures, and for the compile-input audit when one is required. The conditions
//! themselves come from `save_condition`, for every shape of job.

use velnor_model::{CargoCommand, HelperCommand, Job, Step, StepKind, When};

use crate::gate::Setup;
use crate::save_condition::{Audited, Built, Compiled, audit_passed, audit_runs, builds_finished};
use crate::tests::Tests;

const BUILD_ID: &str = "build";
const AUDIT_ID: &str = "audit";

/// The step ids a job's saves wait for, and the ids those steps are given.
#[derive(Clone, Debug, Default)]
pub(crate) struct SaveGate {
    ids: Vec<Option<String>>,
    required: Vec<String>,
    /// What an audit waits for before it runs, by step index.
    audit_runs: Vec<Option<String>>,
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

/// A test step that records its build in its outputs, which the saves read.
fn is_test_run(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Cargo(CargoCommand::NextestRun { .. }))
}

impl SaveGate {
    /// A job without a compile step before its saves keeps the implicit `success()`.
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
        // Only a job whose pinned helper records the build (`Job::records_build`) and that has
        // no build step of its own saves after its test steps (`built` output). Any other job
        // keeps the build steps, whatever steps it happens to have.
        let legacy = !job.records_build
            || job
                .steps
                .iter()
                .any(|step| matches!(&step.kind, StepKind::Cargo(CargoCommand::NextestBuild)));
        let tests = Tests::of(job);
        let restores = crate::base_steps::job_setup(job).0;
        let (mut compiled, mut audits) = (Vec::<Compiled>::new(), Vec::new());
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
        if compiled.is_empty() {
            gate.ids.fill(None);
            gate.audit_runs.fill(None);
        } else {
            gate.required = builds_finished(&compiled, saved_here);
            gate.required.extend(audits.iter().map(audit_passed));
        }
        gate
    }

    pub(crate) fn id(&self, index: usize) -> Option<&str> {
        self.ids.get(index)?.as_deref()
    }

    pub(crate) fn required(&self) -> &[String] {
        &self.required
    }

    /// The condition an audit adds to its own: it runs after a failed test, once the
    /// builds before it finished, so a save never follows an audit that did not run.
    pub(crate) fn audit_runs(&self, index: usize) -> Option<&str> {
        self.audit_runs.get(index)?.as_deref()
    }
}

/// The steps a save waits for: the compile steps of its job, or the whole job when
/// it has none.
pub(crate) fn settled(saved_after: &[String]) -> Vec<String> {
    if saved_after.is_empty() {
        vec!["success()".to_owned()]
    } else {
        saved_after.to_vec()
    }
}
