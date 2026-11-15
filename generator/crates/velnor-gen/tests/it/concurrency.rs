// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::golden::{load, samples};

type TestResult = Result<(), Box<dyn Error>>;

fn cancel_in_progress_value(workflow: &str) -> Result<&str, String> {
    workflow
        .lines()
        .find_map(|line| line.strip_prefix("  cancel-in-progress: "))
        .ok_or_else(|| "no cancel-in-progress under concurrency".to_owned())
}

fn cancels_for(value: &str, event: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => {
            let quoted = value
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
                .ok_or_else(|| format!("not a quoted expression: {value}"))?;
            let expression = quoted
                .replace("''", "'")
                .strip_prefix("${{ ")
                .and_then(|rest| rest.strip_suffix(" }}"))
                .map(str::to_owned)
                .ok_or_else(|| format!("not an expression: {quoted}"))?;
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

#[test]
fn only_a_pull_request_run_cancels_an_older_run_of_its_group() -> TestResult {
    let files = expected_files(&load(&samples().join("velnor-new"))?)?;
    let workflow = files
        .get(&RepoPath::new(".github/workflows/rust-ci.yml")?)
        .ok_or("no generated workflow")?;
    let value = cancel_in_progress_value(workflow)?;
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
