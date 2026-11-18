// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::release_fixture::{jobs, release_view, steps};
use crate::yaml_tree::{Node, parse};

type TestResult = Result<(), Box<dyn Error>>;

const FILE: &str = ".github/workflows/native-checks.yml";

const CONFIG: &str = r#"
[[workflow]]
file = "native-checks"
name = "Native checks"

[[workflow.input]]
name = "sdk_version"
description = "SDK version to check"

[[workflow.input]]
name = "transition_id"
description = "Transition to check"

[[workflow.job]]
id = "plan"
helper = true
timeout_minutes = 15
outputs = { matrix = "steps.plan.outputs.matrix" }

[[workflow.job.step]]
id = "plan"
name = "Plan the checks"
run = "velnor-ci plan"
env = { SDK_VERSION = "${{ inputs.sdk_version }}", GH_TOKEN = "${{ github.token }}" }

[[workflow.job]]
id = "check"
needs = ["plan"]
timeout_minutes = 30

[workflow.job.matrix]
from_output = "plan.matrix"
fail_fast = false
max_parallel = 2

[[workflow.job.step]]
name = "Warm the cache"
run = "velnor-ci warm"
continue_on_error = true
env = { TARGET = "${{ matrix.target }}" }

[[workflow.job.step]]
name = "Run the check"
run = "velnor-ci check"
"#;

fn view_with(config: Option<&str>) -> Result<velnor_model::RepoView, Box<dyn Error>> {
    let mut view = release_view(&[], None)?;
    if let Some(text) = config {
        view.files.insert(
            RepoPath::new(".velnor/workflows.toml")?,
            text.as_bytes().to_vec(),
        );
    }
    Ok(view)
}

fn rendered(config: &str) -> Result<Node, Box<dyn Error>> {
    let mut files = expected_files(&view_with(Some(config))?)?;
    let text = files.remove(&RepoPath::new(FILE)?).ok_or("not rendered")?;
    Ok(parse(&text)?)
}

fn refusal(config: &str) -> Result<String, Box<dyn Error>> {
    match expected_files(&view_with(Some(config))?) {
        Err(error) => Ok(error.to_string()),
        Ok(_) => Err("expected a refusal".into()),
    }
}

#[test]
fn without_the_config_no_reusable_workflow_is_written() -> TestResult {
    let files = expected_files(&view_with(None)?)?;
    assert!(!files.contains_key(&RepoPath::new(FILE)?));
    Ok(())
}

#[test]
fn a_workflow_call_trigger_declares_typed_string_inputs() -> TestResult {
    let tree = rendered(CONFIG)?;
    let inputs = tree
        .at(&["on", "workflow_call", "inputs"])
        .ok_or("no workflow_call inputs")?;
    assert_eq!(inputs.keys(), ["sdk_version", "transition_id"]);
    let input = inputs.get("sdk_version").ok_or("no sdk_version")?;
    assert_eq!(input.get("type").and_then(Node::text), Some("string"));
    assert_eq!(input.get("required").and_then(Node::text), Some("true"));
    assert_eq!(tree.get("on").map(Node::keys), Some(vec!["workflow_call"]));
    Ok(())
}

#[test]
fn a_matrix_can_be_built_from_a_job_output() -> TestResult {
    let tree = rendered(CONFIG)?;
    let all = jobs(&tree)?;
    let plan = all.iter().find(|(id, _)| *id == "plan").ok_or("no plan")?.1;
    assert_eq!(
        plan.at(&["outputs", "matrix"]).and_then(Node::text),
        Some("${{ steps.plan.outputs.matrix }}")
    );
    let check = all
        .iter()
        .find(|(id, _)| *id == "check")
        .ok_or("no check")?
        .1;
    let strategy = check.get("strategy").ok_or("no strategy")?;
    assert_eq!(
        strategy.get("matrix").and_then(Node::text),
        Some("${{ fromJSON(needs.plan.outputs.matrix) }}")
    );
    assert_eq!(
        strategy.get("fail-fast").and_then(Node::text),
        Some("false")
    );
    assert_eq!(strategy.get("max-parallel").and_then(Node::text), Some("2"));
    Ok(())
}

#[test]
fn a_step_can_continue_on_error_and_only_that_step() -> TestResult {
    let tree = rendered(CONFIG)?;
    let all = jobs(&tree)?;
    let check = all
        .iter()
        .find(|(id, _)| *id == "check")
        .ok_or("no check")?
        .1;
    let flags: Vec<Option<&str>> = steps(check)
        .iter()
        .map(|step| step.get("continue-on-error").and_then(Node::text))
        .collect();
    assert_eq!(flags, [Some("true"), None]);
    Ok(())
}

#[test]
fn a_matrix_from_the_output_of_a_job_that_is_not_needed_is_refused() -> TestResult {
    let config = CONFIG.replace("needs = [\"plan\"]\n", "");
    assert!(refusal(&config)?.contains("check"));
    Ok(())
}

#[test]
fn an_expression_over_an_undeclared_input_is_refused() -> TestResult {
    let config = CONFIG.replace("inputs.sdk_version", "inputs.nothing");
    assert!(refusal(&config)?.contains("inputs.nothing"));
    Ok(())
}

#[test]
fn a_matrix_value_outside_a_matrix_job_is_refused() -> TestResult {
    let config = CONFIG.replace("\"${{ github.token }}\"", "\"${{ matrix.target }}\"");
    assert!(refusal(&config)?.contains("matrix.target"));
    Ok(())
}
