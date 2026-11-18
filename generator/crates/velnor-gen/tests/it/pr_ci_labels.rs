// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value as Json;
use velnor_model::RepoPath;

use crate::base_save::pinned;
use crate::events::DENY_MISE;
use crate::facts::TestResult;
use crate::gh_expr::evaluate;
use crate::pr_ci::{LABEL_CONFIG, label_workflow, render};
use crate::pr_ci_sim::{Event, context, labeled, outcome, pull};

#[test]
fn an_unrelated_label_runs_nothing_whatever_the_pull_request_already_carries() -> TestResult {
    let tree = label_workflow()?;
    for labels in [&["bug"][..], &["bug", "ci:run"], &["bug", "ci:auto"]] {
        let (results, _) = outcome(&tree, &labeled("bug", labels), &[])?;
        for (job, result) in &results {
            assert_eq!(result, "skipped", "{labels:?}: {job}");
        }
    }
    Ok(())
}

fn group(tree: &Json, event: &Event) -> Result<String, Box<dyn std::error::Error>> {
    let template = tree
        .pointer("/concurrency/group")
        .and_then(Json::as_str)
        .ok_or("no group")?;
    let results = std::collections::BTreeMap::new();
    let resolve = context(event, &results, &[]);
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("${{") {
        out.push_str(&rest[..start]);
        let end = rest.find("}}").ok_or("unterminated expression")?;
        out.push_str(&evaluate(&rest[start..end + 2], &resolve)?.text());
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

#[test]
fn an_unrelated_label_gets_its_own_concurrency_group_and_cannot_cancel_the_run_in_progress()
-> TestResult {
    let tree = label_workflow()?;
    let running = group(&tree, &pull(false, &["ci:run"]))?;
    assert_eq!(group(&tree, &labeled("ci:run", &["ci:run"]))?, running);
    assert_eq!(group(&tree, &labeled("ci:auto", &["ci:auto"]))?, running);
    assert_eq!(group(&tree, &pull(false, &[]))?, running);
    let unrelated = group(&tree, &labeled("bug", &["bug", "ci:run"]))?;
    assert_ne!(unrelated, running);
    assert!(unrelated.ends_with("-ignored-99"), "{unrelated}");
    assert_eq!(
        tree.pointer("/concurrency/cancel-in-progress")
            .and_then(Json::as_str),
        Some(
            "${{ github.event_name == 'pull_request' && startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed' }}"
        )
    );
    let always = crate::yaml_tree::document(&render("0.1.82", "shape = \"lint-test\"\n")?)?;
    assert!(
        !always
            .pointer("/concurrency/group")
            .and_then(Json::as_str)
            .is_some_and(|text| text.contains("ignored"))
    );
    Ok(())
}

fn every_generated_workflow(
    config: &str,
) -> Result<Vec<(String, Json)>, Box<dyn std::error::Error>> {
    let mut view = pinned("0.1.82", config)?;
    for (path, text) in [("deny.toml", ""), ("mise.toml", DENY_MISE)] {
        let path = RepoPath::new(path)?;
        view.tracked_paths.insert(path.clone());
        view.files.insert(path, text.as_bytes().to_vec());
    }
    crate::facts::lock_pinned_tools(&mut view)?;
    let (files, _) = velnor_gen::expected_with_notes(&view)?;
    files
        .into_iter()
        .map(|(path, text)| Ok((path.to_string(), crate::yaml_tree::document(&text)?)))
        .collect()
}

#[test]
fn every_generated_workflow_that_verifies_a_pull_request_also_runs_on_the_merge_queue_in_label_mode()
-> TestResult {
    let files = every_generated_workflow(LABEL_CONFIG)?;
    let verifying: Vec<&(String, Json)> = files
        .iter()
        .filter(|(_, tree)| tree.pointer("/on/pull_request").is_some())
        .collect();
    assert_eq!(
        verifying
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        [".github/workflows/rust-ci.yml"],
        "a new verifying workflow needs a label-mode merge_group check: {:?}",
        files.iter().map(|(path, _)| path).collect::<Vec<_>>()
    );
    for (path, tree) in verifying {
        assert!(tree.pointer("/on/merge_group").is_some(), "{path}");
    }
    Ok(())
}
