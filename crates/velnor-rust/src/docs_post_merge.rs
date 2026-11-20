// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CargoCommand, DocsRun, EventKind, Job, OptionalSteps, RepoFacts, RustCiConfig, SkipReason,
    StepKind, StepNeed, When,
};

pub(crate) fn runs_after_merge(facts: &RepoFacts, kind: &StepKind) -> bool {
    match kind {
        StepKind::Cargo(CargoCommand::Doc | CargoCommand::DocFeatures { .. }) => {
            facts.config.docs_run == DocsRun::PostMerge
        }
        StepKind::Cargo(CargoCommand::CheckAlone { .. }) => {
            facts.config.check_alone_run == DocsRun::PostMerge
        }
        _ => false,
    }
}

pub(crate) fn apply(facts: &RepoFacts, jobs: &mut [Job]) {
    for job in jobs.iter_mut() {
        let after_merge = job
            .steps
            .iter()
            .any(|step| runs_after_merge(facts, &step.kind));
        for step in &mut job.steps {
            if runs_after_merge(facts, &step.kind) {
                step.when = When::NotCancelledPostMerge;
                step.skip_after_merge_group = None;
            } else if after_merge && step.kind.feeds_build() {
                step.when = When::NotCancelledPostMerge;
            }
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
