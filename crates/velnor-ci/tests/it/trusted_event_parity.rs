// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use velnor_ci::mbx_store::trusted_event;
use velnor_model::{RunnerLabel, TrustedRunners, WORKFLOW_PATH};
use velnor_render::render;
use velnor_test_support::gh_expr::{Value, evaluate};

use crate::sources_fetch_contract::plan;

const NORMAL: &str = "velnor-box";
const TRUSTED: &str = "velnor-box-trusted";

const TABLE: [(&str, &str); 22] = [
    ("push", "refs/heads/main"),
    ("push", "refs/heads/feature"),
    ("push", "refs/heads/main-next"),
    ("push", "refs/heads/release/main"),
    ("push", "refs/tags/main"),
    ("push", "refs/tags/v1.0.0"),
    ("schedule", "refs/heads/main"),
    ("workflow_dispatch", "refs/heads/main"),
    ("workflow_dispatch", "refs/heads/feature"),
    ("workflow_dispatch", "refs/tags/v1.0.0"),
    ("pull_request", "refs/pull/7/merge"),
    ("pull_request", "refs/heads/main"),
    ("pull_request_target", "refs/heads/main"),
    ("pull_request_review", "refs/heads/main"),
    ("merge_group", "refs/heads/gh-readonly-queue/main/pr-7"),
    ("merge_group", "refs/heads/main"),
    ("workflow_run", "refs/heads/main"),
    ("issue_comment", "refs/heads/main"),
    ("repository_dispatch", "refs/heads/main"),
    ("release", "refs/heads/main"),
    ("create", "refs/heads/main"),
    ("delete", "refs/heads/main"),
];

fn label(text: &str) -> RunnerLabel {
    RunnerLabel::new(text).unwrap_or_else(|e| unreachable!("{e}"))
}

fn routing_expression() -> String {
    let trusted = TrustedRunners::new(vec![(label(NORMAL), label(TRUSTED))]);
    let files = render(&plan(&["Cargo.toml"], NORMAL, &[NORMAL], trusted))
        .unwrap_or_else(|e| unreachable!("{e:?}"));
    let text = files
        .get(&WORKFLOW_PATH)
        .unwrap_or_else(|| unreachable!("no workflow"));
    let line = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("runs-on: "))
        .unwrap_or_else(|| unreachable!("no runs-on in {text}"));
    let unquoted = line
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .map_or_else(|| line.to_owned(), |quoted| quoted.replace("''", "'"));
    unquoted
        .strip_prefix("${{ ")
        .and_then(|rest| rest.strip_suffix(" }}"))
        .unwrap_or_else(|| unreachable!("runs-on is not an expression: {line}"))
        .to_owned()
}

fn routes_to_trusted(
    expression: &str,
    event: &str,
    git_ref: &str,
    default_branch: Option<&str>,
    protected: bool,
) -> Result<bool, String> {
    let resolve = |name: &str| -> Result<Value, String> {
        match name {
            "github.event_name" => Ok(Value::Str(event.to_owned())),
            "github.ref" => Ok(Value::Str(git_ref.to_owned())),
            "github.ref_protected" => Ok(Value::Bool(protected)),
            "github.event.repository.default_branch" => {
                Ok(default_branch.map_or(Value::Null, |branch| Value::Str(branch.to_owned())))
            }
            other => Err(format!("unknown context {other}")),
        }
    };
    match evaluate(expression, &resolve)?.text().as_str() {
        TRUSTED => Ok(true),
        NORMAL => Ok(false),
        other => Err(format!("{expression} routed to {other}")),
    }
}

#[test]
fn the_rendered_routing_and_the_helper_trust_the_same_events_on_a_protected_default_branch()
-> Result<(), String> {
    let expression = routing_expression();
    for default_branch in [Some("main"), None, Some("")] {
        for (event, git_ref) in TABLE {
            assert_eq!(
                routes_to_trusted(&expression, event, git_ref, default_branch, true)?,
                trusted_event(event, git_ref, default_branch),
                "{event} {git_ref} {default_branch:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn the_rendered_routing_needs_a_protected_ref_and_the_helper_needs_the_exact_ref()
-> Result<(), String> {
    let expression = routing_expression();
    let rows = [
        ("push", "refs/heads/main", false, false, true),
        ("push", "refs/heads/MAIN", false, false, false),
        ("workflow_dispatch", "refs/heads/MAIN", false, false, false),
        ("schedule", "refs/heads/MAIN", false, false, false),
        ("push", "refs/heads/MAIN", true, true, false),
        ("workflow_dispatch", "refs/heads/MAIN", true, true, false),
        ("push", "refs/heads/main", true, true, true),
    ];
    for (event, git_ref, protected, routed, helper) in rows {
        assert_eq!(
            routes_to_trusted(&expression, event, git_ref, Some("main"), protected)?,
            routed,
            "routing {event} {git_ref} protected={protected}"
        );
        assert_eq!(
            trusted_event(event, git_ref, Some("main")),
            helper,
            "helper {event} {git_ref}"
        );
    }
    Ok(())
}
