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
helper = true
timeout_minutes = 30

[workflow.job.matrix]
from_output = "plan.matrix"
fail_fast = false
max_parallel = 2

[[workflow.job.step]]
name = "Warm the cache"
run = "velnor-ci prepare"
continue_on_error = true
env = { TARGET = "${{ matrix.target }}" }

[[workflow.job.step]]
name = "Run the check"
run = "velnor-ci check-metadata"
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
        .skip(3)
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

fn with_workflow_if(condition: &str) -> String {
    CONFIG.replace(
        "name = \"Native checks\"\n",
        &format!("name = \"Native checks\"\nif = {condition}\n"),
    )
}

fn with_step_if(condition: &str) -> String {
    CONFIG.replace(
        "name = \"Run the check\"\n",
        &format!("name = \"Run the check\"\nif = {condition}\n"),
    )
}

#[test]
fn a_condition_with_a_newline_is_refused_everywhere() -> TestResult {
    for config in [
        with_workflow_if("\"inputs.sdk_version != ''\\n&& true\""),
        with_step_if("\"github.event_name == 'push'\\n&& true\""),
        with_workflow_if("\"inputs.sdk_version != ''\\r\""),
    ] {
        let message = refusal(&config)?;
        assert!(message.contains("line break"), "{message}");
    }
    Ok(())
}

#[test]
fn a_workflow_if_cannot_read_matrix_steps_or_job() -> TestResult {
    for reference in ["matrix.target", "steps.plan.outcome", "job.status"] {
        let message = refusal(&with_workflow_if(&format!("\"{reference} == 'x'\"")))?;
        assert!(message.contains(reference), "{message}");
    }
    Ok(())
}

#[test]
fn a_workflow_if_can_read_inputs_needs_and_github() -> TestResult {
    let tree = rendered(&with_workflow_if(
        "\"inputs.sdk_version != '' && github.event_name == 'push'\"",
    ))?;
    let all = jobs(&tree)?;
    for (_, job) in &all {
        assert!(job.get("if").and_then(Node::text).is_some());
    }
    Ok(())
}

#[test]
fn a_velnor_ci_run_step_needs_helper_on_its_job() -> TestResult {
    let config = CONFIG.replace(
        "needs = [\"plan\"]\nhelper = true\n",
        "needs = [\"plan\"]\n",
    );
    let message = refusal(&config)?;
    assert!(message.contains("helper = true"), "{message}");
    Ok(())
}

/// The `check` job without `helper = true`, whose second step runs `command`.
fn check_job_running(command: &str) -> String {
    CONFIG
        .replace(
            "needs = [\"plan\"]\nhelper = true\n",
            "needs = [\"plan\"]\n",
        )
        .replace("run = \"velnor-ci prepare\"", "run = \"echo warm\"")
        .replace(
            "run = \"velnor-ci check-metadata\"",
            &format!("run = '''{command}'''"),
        )
}

#[test]
fn a_velnor_ci_command_is_refused_without_helper_however_it_is_written() -> TestResult {
    for command in [
        "velnor-ci check",
        "/usr/local/bin/velnor-ci check",
        "../velnor-ci check",
        "env velnor-ci check",
        "env FOO=1 velnor-ci check",
        "VAR=1 velnor-ci check",
        "A=1 B=2 velnor-ci check",
        "'velnor-ci' check",
        "\"/usr/bin/velnor-ci\" check",
        "A=\"x y\" velnor-ci check",
    ] {
        let message = refusal(&check_job_running(command))
            .map_err(|error| format!("{command:?}: {error}"))?;
        assert!(message.contains("helper = true"), "{command:?}: {message}");
    }
    Ok(())
}

#[test]
fn a_command_that_only_mentions_velnor_ci_needs_no_helper() -> TestResult {
    for command in [
        "echo velnor-ci-notes",
        "echo velnor-ci",
        "cargo run -p velnor-ci -- check",
        "velnor-cix check",
        "./velnor-ci-extra check",
        "./velnor-ci check",
        "target/release/velnor-ci check",
    ] {
        rendered(&check_job_running(command)).map_err(|error| format!("{command:?}: {error}"))?;
    }
    Ok(())
}

#[test]
fn an_oversized_reusable_workflow_is_refused_with_its_file_and_largest_jobs() -> TestResult {
    let arguments = "x".repeat(1900);
    let mut config = CONFIG.to_owned();
    for index in 0..300 {
        config.push_str(&format!(
            "\n[[workflow.job.step]]\nname = \"Check {index}\"\nrun = \"velnor-ci check-metadata {arguments}\"\n"
        ));
    }
    let message = refusal(&config)?;
    assert!(message.contains("above 500000 bytes"), "{message}");
    assert!(
        message.contains(&format!("{FILE} has 2 jobs; the largest are check ")),
        "{message}"
    );
    assert!(message.contains(", plan "), "{message}");
    Ok(())
}
