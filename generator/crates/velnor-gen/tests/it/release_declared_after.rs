// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::{RELEASE_WORKFLOW_PATH, RepoPath};

use crate::release_fixture::{APP, release_view};
use crate::yaml_tree::parse;

type TestResult = Result<(), Box<dyn Error>>;

const DECLARED: &str = r#"
[[workflow]]
file = "declared"
name = "Declared"

[workflow.on.push]
branches = ["main"]

[[workflow.job]]
id = "check"

[[workflow.job.step]]
name = "Check"
run = "echo ok"
"#;

fn view(after: &str) -> Result<velnor_model::RepoView, Box<dyn Error>> {
    let mut view = release_view(&[APP], Some(&format!("after = [{after}]\n")))?;
    view.files.insert(
        RepoPath::new(".velnor/workflows.toml")?,
        DECLARED.as_bytes().to_vec(),
    );
    Ok(view)
}

#[test]
fn a_declared_push_workflow_can_be_named_in_the_release_after_list() -> TestResult {
    let files = expected_files(&view("\"Rust CI\", \"Declared\"")?)?;
    let release = files
        .get(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release workflow")?;
    let tree = parse(release)?;
    assert_eq!(
        tree.at(&["on", "workflow_run", "workflows"])
            .ok_or("no workflow_run workflows")?
            .texts(),
        ["Rust CI", "Declared"]
    );
    Ok(())
}

#[test]
fn a_declared_push_workflow_left_out_of_after_is_refused() -> TestResult {
    let error = expected_files(&view("\"Rust CI\"")?)
        .err()
        .ok_or("expected a refusal")?
        .to_string();
    assert!(error.contains("Declared"), "{error}");
    Ok(())
}
