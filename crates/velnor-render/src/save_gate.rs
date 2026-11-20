// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! What the saves of the target and sources caches wait for.
//!
//! A save does not wait for the whole job to succeed: a failing test must not cost
//! the next run its warm target. It waits for the steps that compile the build it
//! captures, and for the compile-input audit when one ran before it.

use velnor_model::{HelperCommand, Job, Step, StepKind, When};

const BUILD_ID: &str = "build";
const AUDIT_ID: &str = "audit";

/// The step ids a job's saves wait for, and the ids those steps are given.
#[derive(Clone, Debug, Default)]
pub(crate) struct SaveGate {
    ids: Vec<Option<String>>,
    required: Vec<String>,
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

impl SaveGate {
    /// A job without a compile step before its saves keeps the implicit `success()`.
    pub(crate) fn of(job: &Job) -> Self {
        let mut gate = Self {
            ids: vec![None; job.steps.len()],
            required: Vec::new(),
        };
        let Some(first) = job.steps.iter().position(is_save) else {
            return gate;
        };
        let saved_here = &job.steps[first].gate;
        let (mut builds, mut audits) = (0, 0);
        let mut required = Vec::new();
        for (index, step) in job.steps.iter().enumerate().take(first) {
            let (prefix, count) = if is_build(step) {
                (BUILD_ID, &mut builds)
            } else if is_audit(step) {
                (AUDIT_ID, &mut audits)
            } else {
                continue;
            };
            *count += 1;
            let id = format!("{prefix}_{count}");
            // A build of another root is skipped, not failed, when the plan leaves
            // that root out; only a build of the saved root must have run.
            required.push(if prefix == BUILD_ID && &step.gate == saved_here {
                format!("steps.{id}.outcome == 'success'")
            } else {
                format!("steps.{id}.outcome != 'failure'")
            });
            gate.ids[index] = Some(id);
        }
        if builds == 0 {
            gate.ids.fill(None);
        } else {
            gate.required = required;
        }
        gate
    }

    pub(crate) fn id(&self, index: usize) -> Option<&str> {
        self.ids.get(index)?.as_deref()
    }

    pub(crate) fn required(&self) -> &[String] {
        &self.required
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
