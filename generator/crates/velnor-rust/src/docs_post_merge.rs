// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CargoCommand, DocsRun, Job, RepoFacts, StepKind, When};

pub(crate) fn apply(facts: &RepoFacts, jobs: &mut [Job]) {
    let docs = facts.config.docs_run == DocsRun::PostMerge;
    let alone = facts.config.check_alone_run == DocsRun::PostMerge;
    for step in jobs.iter_mut().flat_map(|job| job.steps.iter_mut()) {
        let deferred = match step.kind {
            StepKind::Cargo(CargoCommand::Doc) => docs,
            StepKind::Cargo(CargoCommand::CheckAlone { .. }) => alone,
            _ => false,
        };
        if deferred {
            step.when = When::NotCancelledPostMerge;
        }
    }
}
