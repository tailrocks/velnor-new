// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::golden::{load, samples};

type TestResult = Result<(), Box<dyn Error>>;

// The sample is the repository itself, and the repository opts into
// `pr_ci = "label"` (#1476), so its rust-ci.yml renders the label-mode
// concurrency (`CancelOlderPullRequestRunsExceptUnrelatedLabels`). That mode
// gives an edit that keeps the base its own group through the `-ignored-`
// suffix instead of a `cancel-in-progress` exception; `pr_ci_labels.rs` pins it.
fn rust_ci_workflow() -> Result<String, Box<dyn Error>> {
    let files = expected_files(&load(&samples().join("velnor-new"))?)?;
    let workflow = files
        .get(&RepoPath::new(".github/workflows/rust-ci.yml")?)
        .ok_or("no generated workflow")?;
    Ok(workflow.clone())
}

#[test]
fn only_a_pull_request_not_known_closed_can_cancel_an_older_run() -> TestResult {
    let tree = crate::yaml_tree::document(&rust_ci_workflow()?)?;
    assert_eq!(
        tree.pointer("/concurrency/cancel-in-progress")
            .and_then(serde_json::Value::as_str),
        Some(
            "${{ github.event_name == 'pull_request' && startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed' }}"
        ),
        "closed PRs and non-PR events must not cancel open PR work"
    );
    Ok(())
}

#[test]
fn closed_pull_requests_and_non_pull_request_runs_use_their_unique_run_id() -> TestResult {
    let tree = crate::yaml_tree::document(&rust_ci_workflow()?)?;
    assert_eq!(
        tree.pointer("/concurrency/group")
            .and_then(serde_json::Value::as_str),
        Some(
            "ci-${{ github.workflow_ref }}-${{ github.event_name == 'pull_request' && startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed' && !(github.event.action == 'edited' && github.event.changes.base == null) && github.event.pull_request.number || github.run_id }}${{ ((github.event.action == 'labeled' && github.event.label.name != 'ci:run' && github.event.label.name != 'ci:auto') || (github.event.action == 'edited' && github.event.changes.base == null)) && format('-ignored-{0}', github.run_id) || '' }}"
        ),
        "closed PR edits, merged PRs with empty payloads and non-PR events must not replace pending open PR work"
    );
    Ok(())
}

#[test]
fn an_edit_that_keeps_the_base_branch_neither_cancels_nor_shares_the_pull_request_group()
-> TestResult {
    let text = rust_ci_workflow()?;
    let tree = crate::yaml_tree::document(&text)?;
    let edit = "github.event.action == 'edited' && github.event.changes.base == null";
    let group = tree
        .pointer("/concurrency/group")
        .and_then(serde_json::Value::as_str)
        .ok_or("missing concurrency group")?;
    assert!(group.contains(&format!("!({edit})")), "group: {group}");
    assert!(
        group.contains(&format!("({edit})) && format('-ignored-{{0}}'")),
        "an edit that keeps the base must get its own ignored group: {group}"
    );
    assert!(
        text.contains("      - edited\n"),
        "a base retarget must still re-plan"
    );
    Ok(())
}
