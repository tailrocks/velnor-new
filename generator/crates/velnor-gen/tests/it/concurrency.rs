// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::golden::{load, samples};

type TestResult = Result<(), Box<dyn Error>>;

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
            "${{ github.event_name == 'pull_request' && startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed' && !(github.event.action == 'edited' && github.event.changes.base == null) }}"
        ),
        "closed PR edits, merged PRs with empty payloads and non-PR events must not cancel open PR work"
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
            "ci-${{ github.workflow_ref }}-${{ github.event_name == 'pull_request' && startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed' && !(github.event.action == 'edited' && github.event.changes.base == null) && github.event.pull_request.number || github.run_id }}"
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
    for pointer in ["/concurrency/group", "/concurrency/cancel-in-progress"] {
        let value = tree
            .pointer(pointer)
            .and_then(serde_json::Value::as_str)
            .ok_or("missing concurrency expression")?;
        assert!(value.contains(&format!("!({edit})")), "{pointer}: {value}");
    }
    assert!(
        text.contains("      - edited\n"),
        "a base retarget must still re-plan"
    );
    Ok(())
}
