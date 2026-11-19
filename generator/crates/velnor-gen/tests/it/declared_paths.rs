// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{JOB, TestResult, refusal, rendered, workflow};
use crate::yaml_tree::Node;

fn with_on(on: &str) -> String {
    workflow("", on, JOB)
}

fn texts<'a>(tree: &'a Node, path: &[&str]) -> Option<Vec<&'a str>> {
    tree.at(path).map(Node::texts)
}

#[test]
fn push_and_pull_request_take_paths_and_render_them_after_the_branches() -> TestResult {
    let tree = rendered(&with_on(
        "[workflow.on.push]\nbranches = [\"main\"]\npaths = [\"crates/**\", \"Cargo.lock\"]\n\n[workflow.on.pull_request]\ntypes = [\"opened\"]\nbranches = [\"main\"]\npaths = [\"crates/**\"]\n",
    ))?;
    assert_eq!(
        texts(&tree, &["on", "push", "paths"]),
        Some(vec!["crates/**", "Cargo.lock"])
    );
    assert_eq!(
        texts(&tree, &["on", "pull_request", "paths"]),
        Some(vec!["crates/**"])
    );
    let push = tree.at(&["on", "push"]).ok_or("no push")?;
    assert_eq!(push.keys(), ["branches", "paths"]);
    let request = tree.at(&["on", "pull_request"]).ok_or("no pull_request")?;
    assert_eq!(request.keys(), ["types", "branches", "paths"]);
    assert!(push.get("paths-ignore").is_none());
    Ok(())
}

#[test]
fn push_and_pull_request_take_paths_ignore_and_write_it_as_paths_ignore() -> TestResult {
    let tree = rendered(&with_on(
        "[workflow.on.push]\ntags = [\"v*\"]\npaths_ignore = [\"docs/**\", \"**.md\"]\n\n[workflow.on.pull_request]\ntypes = [\"opened\"]\npaths_ignore = [\"docs/**\"]\n",
    ))?;
    assert_eq!(
        texts(&tree, &["on", "push", "paths-ignore"]),
        Some(vec!["docs/**", "**.md"])
    );
    assert_eq!(
        texts(&tree, &["on", "pull_request", "paths-ignore"]),
        Some(vec!["docs/**"])
    );
    let push = tree.at(&["on", "push"]).ok_or("no push")?;
    assert_eq!(push.keys(), ["tags", "paths-ignore"]);
    assert!(push.get("paths").is_none());
    Ok(())
}

#[test]
fn paths_and_paths_ignore_exclude_each_other_on_one_trigger_but_not_across_two() -> TestResult {
    for on in [
        "[workflow.on.push]\nbranches = [\"main\"]\npaths = [\"a/**\"]\npaths_ignore = [\"b/**\"]\n",
        "[workflow.on.pull_request]\ntypes = [\"opened\"]\npaths = [\"a/**\"]\npaths_ignore = [\"b/**\"]\n",
    ] {
        let message = refusal(&with_on(on))?;
        assert!(
            message.contains("paths") && message.contains("paths_ignore"),
            "{on}: {message}"
        );
    }
    rendered(&with_on(
        "[workflow.on.push]\nbranches = [\"main\"]\npaths = [\"a/**\"]\n\n[workflow.on.pull_request]\ntypes = [\"opened\"]\npaths_ignore = [\"b/**\"]\n",
    ))?;
    Ok(())
}

#[test]
fn a_path_pattern_that_is_malformed_or_redundant_is_refused() -> TestResult {
    let cases = [
        ("paths = [\"\"]", "paths"),
        ("paths = [\"a\", \"a\"]", "paths"),
        ("paths = [\"a/${{ github.sha }}\"]", "paths"),
        ("paths_ignore = [\"has space\"]", "paths_ignore"),
        ("paths = [\"**\"]", "redundant"),
        ("paths_ignore = [\"**\"]", "every file"),
    ];
    for (filter, needle) in cases {
        for trigger in [
            format!("[workflow.on.push]\nbranches = [\"main\"]\n{filter}\n"),
            format!("[workflow.on.pull_request]\ntypes = [\"opened\"]\n{filter}\n"),
        ] {
            let message = refusal(&with_on(&trigger))?;
            assert!(message.contains(needle), "{trigger}: {message}");
        }
    }
    let run = "[workflow.on.workflow_run]\nworkflows = [\"CI\"]\npaths = [\"a/**\"]\n";
    assert!(refusal(&with_on(run))?.contains("paths"));
    Ok(())
}

#[test]
fn a_branch_filter_of_every_branch_is_refused_as_redundant() -> TestResult {
    let cases = [
        "[workflow.on.push]\nbranches = [\"**\"]\n",
        "[workflow.on.push]\nbranches = [\"main\", \"**\"]\n",
        "[workflow.on.pull_request]\ntypes = [\"opened\"]\nbranches = [\"**\"]\n",
        "[workflow.on.workflow_run]\nworkflows = [\"CI\"]\nbranches = [\"**\"]\n",
    ];
    for on in cases {
        let message = refusal(&with_on(on))?;
        assert!(
            message.contains("redundant") && message.contains("omit"),
            "{on}: {message}"
        );
    }
    rendered(&with_on(
        "[workflow.on.push]\nbranches = [\"*\", \"releases/**\"]\n",
    ))?;
    rendered(&with_on("[workflow.on.push]\ntags = [\"**\"]\n"))?;
    Ok(())
}

#[test]
fn a_schedule_faster_than_every_five_minutes_is_refused() -> TestResult {
    let refused = [
        "* * * * *",
        "*/1 * * * *",
        "*/2 * * * *",
        "*/4 * * * *",
        "0-30/3 * * * *",
        "0,3 * * * *",
        "58,59 * * * *",
        "*/7 * * * *",
        "0/4 * * * *",
        "*/3 6 * * 1",
        "0,57 0,23 * * *",
        "0,57 0,23 * * 1,2",
        "0,57 0,23 1,2 * *",
        "0,57 0,23 1 * 2",
        "0,57 0,23 * 1-2 *",
    ];
    for cron in refused {
        let message = refusal(&with_on(&format!(
            "[workflow.on]\nschedule = [\"{cron}\"]\n"
        )))?;
        assert!(
            message.contains(cron) && message.contains("5 minutes"),
            "{cron}: {message}"
        );
    }
    let allowed = [
        "0,57 0,23 * * 1",
        "0,57 0,23 1 * *",
        "0,57 0,23 29 2 *",
        "0,57 0,23 * * 1,3",
        "*/5 * * * *",
        "*/10 * * * *",
        "*/15 * * * *",
        "0,5 * * * *",
        "0 * * * *",
        "23 4 * * 1",
        "0,30 6 * * *",
        "59 23 * * *",
        "0 0,1 * * *",
        "5-59/5 * * * *",
    ];
    for cron in allowed {
        rendered(&with_on(&format!(
            "[workflow.on]\nschedule = [\"{cron}\"]\n"
        )))?;
    }
    Ok(())
}

#[test]
fn a_schedule_no_day_matches_is_refused() -> TestResult {
    for cron in ["0 0 31 2 *", "0 0 30 2 *", "0 0 31 4,6,9,11 *"] {
        let message = refusal(&with_on(&format!(
            "[workflow.on]\nschedule = [\"{cron}\"]\n"
        )))?;
        assert!(
            message.contains(cron) && message.contains("never runs"),
            "{cron}: {message}"
        );
    }
    Ok(())
}

#[test]
fn a_filter_of_only_negative_patterns_is_refused() -> TestResult {
    let cases = [
        "[workflow.on.push]\nbranches = [\"!release\"]\n",
        "[workflow.on.push]\nbranches = [\"main\"]\ntags = [\"!v1\"]\n",
        "[workflow.on.push]\nbranches = [\"main\"]\npaths = [\"!docs/**\"]\n",
        "[workflow.on.pull_request]\ntypes = [\"opened\"]\nbranches = [\"!wip\", \"!old\"]\n",
        "[workflow.on.pull_request]\ntypes = [\"opened\"]\npaths = [\"!docs/**\"]\n",
        "[workflow.on.workflow_run]\nworkflows = [\"CI\"]\nbranches = [\"!wip\"]\n",
    ];
    for on in cases {
        let message = refusal(&with_on(on))?;
        assert!(
            message.contains("only negative patterns"),
            "{on}: {message}"
        );
    }
    rendered(&with_on(
        "[workflow.on.push]\nbranches = [\"main\", \"!release\"]\ntags = [\"v*\", \"!v1\"]\npaths = [\"src/**\", \"!src/gen/**\"]\n",
    ))?;
    Ok(())
}
