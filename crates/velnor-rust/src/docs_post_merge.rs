// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::doc_steps::documented;
use velnor_model::{
    CargoCommand, DocsRun, EventKind, Job, OptionalSteps, RepoFacts, RepoPath, RustCiConfig,
    RustSelection, SkipReason, StepKind, StepNeed, When,
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
        let pre_merge_build = job
            .steps
            .iter()
            .any(|step| !runs_after_merge(facts, &step.kind) && compiles(&step.kind));
        let defer_inputs = after_merge && !pre_merge_build;
        for step in &mut job.steps {
            if runs_after_merge(facts, &step.kind) {
                step.when = When::NotCancelledPostMerge;
                step.skip_after_merge_group = None;
            } else if defer_inputs && step.kind.feeds_build() {
                step.when = When::NotCancelledPostMerge;
            }
        }
    }
}

fn compiles(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Cargo(command) if command.builds())
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

#[must_use]
pub fn defer_unselected_docs(
    facts: &RepoFacts,
    event: EventKind,
    root: &RepoPath,
    selection: &RustSelection,
    steps: OptionalSteps,
) -> OptionalSteps {
    let before_merge = matches!(event, EventKind::PullRequest | EventKind::MergeGroup);
    if facts.config.docs_run != DocsRun::Selected || !before_merge || steps.docs != StepNeed::Run {
        return steps;
    }
    let RustSelection::Some(selected) = selection else {
        return steps;
    };
    let reaches_documented = facts
        .rust_roots
        .iter()
        .filter(|candidate| &candidate.manifest == root)
        .flat_map(|candidate| &candidate.members)
        .filter(|member| selected.contains_key(&member.name))
        .any(|member| {
            member
                .targets
                .iter()
                .flat_map(|target| &target.kinds)
                .any(|kind| documented(*kind))
        });
    if reaches_documented {
        steps
    } else {
        OptionalSteps {
            docs: StepNeed::Skip(SkipReason::NoSelectedDocumented),
            ..steps
        }
    }
}
