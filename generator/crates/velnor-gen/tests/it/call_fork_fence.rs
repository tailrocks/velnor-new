// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, job, rendered_file};
use crate::yaml_tree::Node;

pub(crate) const FENCE: &str = "(!startsWith(github.event_name, 'pull_request') || github.event.pull_request.head.repo.full_name == github.repository)";

const LOCAL: &str = "./.github/workflows/hand-written.yml";
const PINNED: &str =
    "tailrocks/velnor-new/.github/workflows/x.yml@0123456789abcdef0123456789abcdef01234567";

/// The fence, as the only condition of a call job, or its first conjunct.
pub(crate) fn fence_expression() -> String {
    format!("${{{{ {FENCE} }}}}")
}

/// Evaluates the fence for an event: the same condition GitHub evaluates for the job.
fn fence_allows(event_name: &str, head_repository: &str, repository: &str) -> bool {
    let is_pull_request = event_name.starts_with("pull_request");
    let same = head_repository == repository;
    let substituted = FENCE
        .replace(
            "startsWith(github.event_name, 'pull_request')",
            &is_pull_request.to_string(),
        )
        .replace(
            "github.event.pull_request.head.repo.full_name == github.repository",
            &same.to_string(),
        );
    substituted
        .trim_matches(|c| c == '(' || c == ')')
        .split("||")
        .any(|term| match term.trim() {
            "true" => true,
            "false" => false,
            "!true" => false,
            "!false" => true,
            other => unreachable!("{other}"),
        })
}

/// A callable workflow with one call job of each callee kind, one with an `if`, and a declared leaf.
fn config(on: &str) -> String {
    format!(
        "[[workflow]]\nfile = \"leaf\"\nname = \"Leaf\"\n\n[[workflow.job]]\nid = \"build\"\ntimeout_minutes = 5\n\n[[workflow.job.step]]\nname = \"Build\"\nrun = \"echo ok\"\n\n[[workflow]]\nfile = \"caller\"\nname = \"Caller\"\nrunner = \"velnor-linux-x64\"\n\n{on}\n\n[[workflow.job]]\nid = \"local\"\nuses = \"{LOCAL}\"\n\n[[workflow.job]]\nid = \"pinned\"\nuses = \"{PINNED}\"\n\n[[workflow.job]]\nid = \"declared\"\nuses = \"leaf\"\n\n[[workflow.job]]\nid = \"guarded\"\nuses = \"leaf\"\nif = \"github.ref == 'refs/heads/main'\"\n"
    )
}

#[test]
fn the_fence_lets_a_same_repository_event_through_and_stops_a_fork_pull_request() {
    for event in ["push", "workflow_dispatch", "schedule", "merge_group"] {
        assert!(fence_allows(event, "", "a/b"), "{event}");
    }
    for event in ["pull_request", "pull_request_target"] {
        assert!(fence_allows(event, "a/b", "a/b"), "{event}");
        assert!(!fence_allows(event, "fork/b", "a/b"), "{event}");
        assert!(
            !fence_allows(event, "", "a/b"),
            "{event} from a deleted fork"
        );
    }
}

#[test]
fn a_call_job_of_every_callee_kind_in_a_callable_workflow_is_skipped_for_a_fork() -> TestResult {
    let tree = rendered_file(&config("[workflow.on]\nworkflow_call = true"), "caller")?;
    for id in ["local", "pinned", "declared"] {
        let condition = job(&tree, id)?
            .get("if")
            .and_then(Node::text)
            .ok_or_else(|| format!("{id} has no if"))?;
        assert_eq!(condition, fence_expression(), "{id}");
    }
    Ok(())
}

#[test]
fn a_declared_call_from_a_pull_request_workflow_is_skipped_for_a_fork() -> TestResult {
    let config = config("[workflow.on.pull_request]\ntypes = [\"opened\"]");
    let config = config
        .split("[[workflow.job]]\nid = \"local\"")
        .next()
        .ok_or("no jobs")?
        .to_owned()
        + "[[workflow.job]]\nid = \"declared\"\nuses = \"leaf\"\n";
    let tree = rendered_file(&config, "caller")?;
    assert_eq!(
        job(&tree, "declared")?.get("if").and_then(Node::text),
        Some(fence_expression().as_str())
    );
    Ok(())
}

#[test]
fn a_call_job_with_its_own_if_gets_one_expression_with_both_conditions() -> TestResult {
    for on in [
        "[workflow.on]\nworkflow_call = true",
        "[workflow.on.push]\nbranches = [\"main\"]",
    ] {
        let tree = rendered_file(&config(on), "caller")?;
        let condition = job(&tree, "guarded")?
            .get("if")
            .and_then(Node::text)
            .ok_or("no if")?;
        assert_eq!(
            condition,
            format!("${{{{ ({FENCE}) && (github.ref == 'refs/heads/main') }}}}"),
            "{on}"
        );
        assert_eq!(condition.matches("${{").count(), 1, "{condition}");
    }
    Ok(())
}

#[test]
fn a_workflow_level_if_joins_the_fence_on_a_call_job() -> TestResult {
    let config = config("[workflow.on.push]\nbranches = [\"main\"]").replace(
        "runner = \"velnor-linux-x64\"\n",
        "runner = \"velnor-linux-x64\"\nif = \"github.repository_owner == 'tailrocks'\"\n",
    );
    let tree = rendered_file(&config, "caller")?;
    let condition = job(&tree, "declared")?
        .get("if")
        .and_then(Node::text)
        .ok_or("no if")?;
    assert!(condition.contains(FENCE), "{condition}");
    assert!(
        condition.contains("github.repository_owner == 'tailrocks'"),
        "{condition}"
    );
    assert_eq!(condition.matches("${{").count(), 1, "{condition}");
    Ok(())
}
