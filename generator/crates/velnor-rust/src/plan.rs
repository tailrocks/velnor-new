// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    Access, CargoCommand, Concurrency, Job, JobId, PermissionScope, Permissions, RepoFacts, Step,
    StepKind, Triggers, WorkflowPlan,
};

const FORMAT_JOB: JobId = JobId::from_static("format");

#[must_use]
pub fn plan(facts: &RepoFacts) -> WorkflowPlan {
    let mut steps = vec![step(StepKind::Checkout { depth: 1 })];
    steps.extend(
        facts
            .helper
            .iter()
            .map(|release| step(StepKind::FetchHelper(release.clone()))),
    );
    steps.extend(facts.rust_roots.iter().map(|root| {
        step(StepKind::Cargo(CargoCommand::FmtCheck {
            manifest: root.manifest.clone(),
        }))
    }));
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
        jobs: vec![Job {
            id: FORMAT_JOB,
            runs_on: facts.config.runner.clone(),
            timeout_minutes: facts.config.timeout_minutes,
            steps,
        }],
    }
}

fn step(kind: StepKind) -> Step {
    Step { kind }
}
