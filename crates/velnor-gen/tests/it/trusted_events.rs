// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_test_support::gh_expr::{Value, evaluate};

pub(crate) const REPOSITORY: &str = "owner/repo";
pub(crate) const FORK: &str = "stranger/repo";
const DEFAULT_BRANCH: &str = "main";

pub(crate) struct Event {
    pub(crate) name: &'static str,
    pub(crate) git_ref: &'static str,
    pub(crate) head_repository: Option<&'static str>,
    pub(crate) default_branch: Option<&'static str>,
    pub(crate) ref_protected: bool,
}

const fn event(name: &'static str, git_ref: &'static str) -> Event {
    Event {
        name,
        git_ref,
        head_repository: None,
        default_branch: Some(DEFAULT_BRANCH),
        ref_protected: true,
    }
}

const fn unprotected(name: &'static str, git_ref: &'static str) -> Event {
    Event {
        ref_protected: false,
        ..event(name, git_ref)
    }
}

const fn bare(name: &'static str, git_ref: &'static str) -> Event {
    Event {
        name,
        git_ref,
        head_repository: None,
        default_branch: None,
        ref_protected: true,
    }
}

const fn pull(name: &'static str, git_ref: &'static str, head: &'static str) -> Event {
    Event {
        name,
        git_ref,
        head_repository: Some(head),
        default_branch: Some(DEFAULT_BRANCH),
        ref_protected: true,
    }
}

pub(crate) const TRUSTED: &str = "trusted";
pub(crate) const NORMAL: &str = "normal";
pub(crate) const FORK_RUNNER: &str = "fork";

pub(crate) fn events() -> Vec<(Event, &'static str)> {
    vec![
        (event("push", "refs/heads/main"), TRUSTED),
        (event("push", "refs/heads/feature"), NORMAL),
        (unprotected("push", "refs/heads/MAIN"), NORMAL),
        (unprotected("push", "refs/heads/main"), NORMAL),
        (unprotected("schedule", "refs/heads/main"), NORMAL),
        (unprotected("workflow_dispatch", "refs/heads/main"), NORMAL),
        (event("push", "refs/tags/v1.0.0"), NORMAL),
        (event("schedule", "refs/heads/main"), TRUSTED),
        (bare("schedule", "refs/heads/main"), NORMAL),
        (bare("push", "refs/heads/main"), NORMAL),
        (bare("workflow_dispatch", "refs/heads/main"), NORMAL),
        (event("schedule", "refs/heads/feature"), NORMAL),
        (event("workflow_dispatch", "refs/heads/main"), TRUSTED),
        (event("workflow_dispatch", "refs/heads/feature"), NORMAL),
        (
            pull("pull_request", "refs/pull/7/merge", REPOSITORY),
            NORMAL,
        ),
        (pull("pull_request", "refs/pull/7/merge", FORK), FORK_RUNNER),
        (
            pull("pull_request_target", "refs/heads/main", REPOSITORY),
            NORMAL,
        ),
        (
            pull("pull_request_target", "refs/heads/main", FORK),
            FORK_RUNNER,
        ),
        (
            pull("pull_request_review", "refs/pull/7/merge", REPOSITORY),
            NORMAL,
        ),
        (
            event("merge_group", "refs/heads/gh-readonly-queue/main/pr-7"),
            NORMAL,
        ),
        (event("workflow_run", "refs/heads/main"), NORMAL),
        (event("issue_comment", "refs/heads/main"), NORMAL),
        (event("repository_dispatch", "refs/heads/main"), NORMAL),
        (event("release", "refs/heads/main"), NORMAL),
    ]
}

pub(crate) fn runs_on(
    expression: &str,
    event: &Event,
    matrix: &[(&str, &str)],
) -> Result<String, String> {
    let resolve = |name: &str| -> Result<Value, String> {
        let text = |value: &str| Ok(Value::Str(value.to_owned()));
        match name {
            "github.event_name" => text(event.name),
            "github.ref" => text(event.git_ref),
            "github.ref_protected" => Ok(Value::Bool(event.ref_protected)),
            "github.repository" => text(REPOSITORY),
            "github.event.repository.default_branch" => Ok(event
                .default_branch
                .map_or(Value::Null, |branch| Value::Str(branch.to_owned()))),
            "github.event.pull_request.head.repo.full_name" => Ok(event
                .head_repository
                .map_or(Value::Null, |head| Value::Str(head.to_owned()))),
            other => match other.strip_prefix("matrix.") {
                Some(axis) => matrix.iter().find(|(key, _)| *key == axis).map_or_else(
                    || Err(format!("no matrix.{axis}")),
                    |(_, value)| text(value),
                ),
                None => Err(format!("unknown context {other}")),
            },
        }
    };
    Ok(evaluate(expression, &resolve)?.text())
}
