// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CargoCommand, DocsRun, Job, RepoFacts, StepKind, When};

pub(crate) fn apply(facts: &RepoFacts, jobs: &mut [Job]) {
    if facts.config.docs_run != DocsRun::PostMerge {
        return;
    }
    for step in jobs.iter_mut().flat_map(|job| job.steps.iter_mut()) {
        if step.kind == StepKind::Cargo(CargoCommand::Doc) {
            step.when = When::NotCancelledPostMerge;
        }
    }
}
