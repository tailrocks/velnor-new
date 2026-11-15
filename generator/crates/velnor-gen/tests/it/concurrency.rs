// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::golden::{load, samples};

type TestResult = Result<(), Box<dyn Error>>;

const DEFAULT_BRANCH_REF: &str = "refs/heads/main";
const PULL_REQUEST_NUMBER: &str = "7";

fn generated_line<'a>(workflow: &'a str, prefix: &str) -> Result<&'a str, String> {
    workflow
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .ok_or_else(|| format!("no {prefix} under concurrency"))
}

fn unquote(value: &str) -> Result<String, String> {
    value
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .map(|rest| rest.replace("''", "'"))
        .ok_or_else(|| format!("not a quoted expression: {value}"))
}

fn cancels_for(value: &str, event: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => {
            let expression = unquote(value)?
                .strip_prefix("${{ ")
                .and_then(|rest| rest.strip_suffix(" }}"))
                .map(str::to_owned)
                .ok_or_else(|| format!("not an expression: {value}"))?;
            let (left, right) = expression
                .split_once(" == ")
                .ok_or_else(|| format!("not a comparison: {expression}"))?;
            if left != "github.event_name" {
                return Err(format!("unexpected operand: {left}"));
            }
            Ok(right.trim_matches('\'') == event)
        }
    }
}

fn group_for(group: &str, event: &str, number: &str, run_id: &str) -> Result<String, String> {
    let mut resolved = String::new();
    for piece in unquote(group)?.split("${{ ").skip(1) {
        let (operand, rest) = piece
            .split_once(" }}")
            .ok_or_else(|| format!("unclosed expression in {group}"))?;
        resolved.push_str(&match operand {
            "github.workflow_ref" => DEFAULT_BRANCH_REF.to_owned(),
            "github.event_name == 'pull_request' && github.event.pull_request.number || github.run_id" => {
                if event == "pull_request" {
                    number.to_owned()
                } else {
                    run_id.to_owned()
                }
            }
            other => return Err(format!("unexpected operand: {other}")),
        });
        resolved.push_str(rest);
    }
    Ok(resolved)
}

fn rust_ci_workflow() -> Result<String, Box<dyn Error>> {
    let files = expected_files(&load(&samples().join("velnor-new"))?)?;
    let workflow = files
        .get(&RepoPath::new(".github/workflows/rust-ci.yml")?)
        .ok_or("no generated workflow")?;
    Ok(workflow.clone())
}

#[test]
fn only_a_pull_request_run_cancels_an_older_run_of_its_group() -> TestResult {
    let workflow = rust_ci_workflow()?;
    let value = generated_line(&workflow, "  cancel-in-progress: ")?;
    for (event, cancels) in [
        ("pull_request", true),
        ("push", false),
        ("merge_group", false),
        ("schedule", false),
        ("workflow_dispatch", false),
    ] {
        assert_eq!(cancels_for(value, event)?, cancels, "{event}: {value}");
    }
    Ok(())
}

#[test]
fn a_run_outside_a_pull_request_never_shares_its_group() -> TestResult {
    let workflow = rust_ci_workflow()?;
    let group = generated_line(&workflow, "  group: ")?;
    for event in ["push", "merge_group", "schedule", "workflow_dispatch"] {
        let first = group_for(group, event, "", "1111111")?;
        let second = group_for(group, event, "", "2222222")?;
        assert_ne!(first, second, "{event}: {group}");
    }
    Ok(())
}

#[test]
fn pull_request_pushes_of_one_pull_request_share_a_group() -> TestResult {
    let workflow = rust_ci_workflow()?;
    let group = generated_line(&workflow, "  group: ")?;
    let first = group_for(group, "pull_request", PULL_REQUEST_NUMBER, "1111111")?;
    let second = group_for(group, "pull_request", PULL_REQUEST_NUMBER, "2222222")?;
    assert_eq!(first, second, "{group}");
    Ok(())
}
