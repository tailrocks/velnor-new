// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CargoCommand, DocsRun, EventKind, Job, OptionalSteps, RepoFacts, RustCiConfig, SkipReason,
    StepKind, StepNeed, When,
};

pub(crate) fn apply(facts: &RepoFacts, jobs: &mut [Job]) {
    let docs = facts.config.docs_run == DocsRun::PostMerge;
    let alone = facts.config.check_alone_run == DocsRun::PostMerge;
    for step in jobs.iter_mut().flat_map(|job| job.steps.iter_mut()) {
        let deferred = match step.kind {
            StepKind::Cargo(CargoCommand::Doc | CargoCommand::DocFeatures { .. }) => docs,
            StepKind::Cargo(CargoCommand::CheckAlone { .. }) => alone,
            _ => false,
        };
        if deferred {
            step.when = When::NotCancelledPostMerge;
            step.skip_after_merge_group = None;
        }
    }
}

#[must_use]
pub fn defer_post_merge_docs(
    config: &RustCiConfig,
    event: EventKind,
    steps: OptionalSteps,
) -> OptionalSteps {
    let before_merge = matches!(event, EventKind::PullRequest | EventKind::MergeGroup);
    if config.docs_run == DocsRun::PostMerge && before_merge {
        OptionalSteps {
            docs: StepNeed::Skip(SkipReason::PostMergeOnly),
            ..steps
        }
    } else {
        steps
    }
}
