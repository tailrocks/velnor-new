// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Concurrency, PullRequestType, Triggers};

use crate::yaml::{Yaml, entry, text};

const PULL_REQUEST_GROUP: &str = "ci-${{ github.workflow_ref }}-${{ github.event_name == 'pull_request' && startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed' && github.event.pull_request.number || github.run_id }}";
const UNRELATED_LABEL_SUFFIX: &str = "${{ github.event.action == 'labeled' && github.event.label.name != 'ci:run' && github.event.label.name != 'ci:auto' && format('-ignored-{0}', github.run_id) || '' }}";
const CANCEL_PULL_REQUEST_RUNS: &str = "${{ github.event_name == 'pull_request' && startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed' }}";

pub(crate) fn concurrency(concurrency: Concurrency) -> Yaml {
    let (group, cancel) = match concurrency {
        Concurrency::CancelOlderPullRequestRuns => {
            (PULL_REQUEST_GROUP.to_owned(), CANCEL_PULL_REQUEST_RUNS)
        }
        Concurrency::CancelOlderPullRequestRunsExceptUnrelatedLabels => (
            format!("{PULL_REQUEST_GROUP}{UNRELATED_LABEL_SUFFIX}"),
            CANCEL_PULL_REQUEST_RUNS,
        ),
    };
    Yaml::Map(vec![
        entry("group", Yaml::Expr(group)),
        entry("cancel-in-progress", Yaml::Expr(cancel.to_owned())),
    ])
}

fn branch_filter(branch: &str) -> String {
    branch.chars().fold(String::new(), |mut out, c| {
        if matches!(c, '+' | '!' | ']') {
            out.push('\\');
        }
        out.push(c);
        out
    })
}

pub(crate) fn triggers(triggers: &Triggers) -> Yaml {
    let mut items = Vec::new();
    if !triggers.pull_request.is_empty() {
        let types = triggers.pull_request.iter().map(|kind| {
            text(match kind {
                PullRequestType::Opened => "opened",
                PullRequestType::Synchronize => "synchronize",
                PullRequestType::Reopened => "reopened",
                PullRequestType::ReadyForReview => "ready_for_review",
                PullRequestType::Edited => "edited",
                PullRequestType::ConvertedToDraft => "converted_to_draft",
                PullRequestType::Labeled => "labeled",
            })
        });
        items.push(entry(
            "pull_request",
            Yaml::Map(vec![entry("types", Yaml::Seq(types.collect()))]),
        ));
    }
    if let Some(branch) = &triggers.push_branch {
        items.push(entry(
            "push",
            Yaml::Map(vec![entry(
                "branches",
                Yaml::Seq(vec![text(&branch_filter(branch))]),
            )]),
        ));
    }
    if triggers.merge_group {
        items.push(entry("merge_group", Yaml::Map(Vec::new())));
    }
    if let Some(cron) = &triggers.schedule {
        items.push(entry(
            "schedule",
            Yaml::Seq(vec![Yaml::Map(vec![entry("cron", text(cron.as_str()))])]),
        ));
    }
    if triggers.workflow_dispatch {
        items.push(entry("workflow_dispatch", Yaml::Map(Vec::new())));
    }
    Yaml::Map(items)
}
