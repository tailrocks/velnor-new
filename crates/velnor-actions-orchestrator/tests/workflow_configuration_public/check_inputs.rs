// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_orchestrator::prepare;

use super::scoped_workspace::{A5_COMPLETE_RULE, TestResult, commit, put, scoped_workspace};

fn check(inputs: &str) -> String {
    format!(
        "\n[stacks.rust]\n{A5_COMPLETE_RULE}\n[[checks]]\nid = \"docs-lint\"\ntask = \"fixture-check\"\ninputs = [{inputs}]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n"
    )
}

fn prepare_with_check(inputs: &str) -> Result<String, Box<dyn Error>> {
    let repo = scoped_workspace(&check(inputs))?;
    put(
        repo.root(),
        "mise.toml",
        "[tasks.fixture-check]\nrun = \"true\"\n",
    )?;
    put(repo.root(), "scripts/lint.sh", "#!/bin/sh\n")?;
    commit(repo.root(), "scenario")?;
    Ok(match prepare(repo.root()) {
        Ok(_) => String::new(),
        Err(error) => error.to_string(),
    })
}

#[test]
fn a_check_input_naming_a_directory_is_rejected_with_the_path() -> TestResult {
    let message = prepare_with_check("\"scripts\"")?;
    assert!(
        message.contains("check_input_is_directory:scripts"),
        "expected the directory input to be named, got: {message:?}"
    );
    Ok(())
}

#[test]
fn a_check_input_naming_a_file_is_accepted() -> TestResult {
    let message = prepare_with_check("\"scripts/lint.sh\"")?;
    assert_eq!(message, "");
    Ok(())
}
