// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A declared job can take its runner from a matrix axis. Every value of the axis must be
//! declared in `runner_labels`, and the job is open to fork pull requests only when every
//! one of them is declared `hosted = true`.

use crate::declared_fixture::{TestResult, job, refusal, rendered, workflow};
use crate::yaml_tree::Node;

const ON_PULL_REQUEST: &str = "[workflow.on.pull_request]\ntypes = [\"opened\"]\n";
const FENCE: &str = "github.event.pull_request.head.repo.full_name == github.repository";

fn build(header: &str, matrix: &str) -> String {
    workflow(
        "",
        ON_PULL_REQUEST,
        &format!(
            "\n[[workflow.job]]\nid = \"build\"\n{header}\n[workflow.job.matrix]\n{matrix}\n[[workflow.job.step]]\nname = \"Build\"\nrun = \"echo ok\"\n"
        ),
    )
}

fn runs_on(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let tree = rendered(config)?;
    Ok(job(&tree, "build")?
        .get("runs-on")
        .and_then(Node::text)
        .ok_or("no runs-on")?
        .to_owned())
}

const AXIS: &str = "axes = { runner = [\"ubuntu-24.04\", \"ubuntu-24.04-arm\"] }\n";

#[test]
fn a_matrix_runner_of_only_hosted_labels_is_open_to_fork_pull_requests() -> TestResult {
    let config = build(
        "runner = \"${{ matrix.runner }}\"\nrunner_labels = { \"ubuntu-24.04\" = true, \"ubuntu-24.04-arm\" = true }",
        AXIS,
    );
    assert_eq!(runs_on(&config)?, "${{ matrix.runner }}");
    let tree = rendered(&config)?;
    assert_eq!(
        job(&tree, "build")?
            .at(&["strategy", "matrix", "runner"])
            .map(Node::texts),
        Some(vec!["ubuntu-24.04", "ubuntu-24.04-arm"])
    );
    Ok(())
}

#[test]
fn a_matrix_runner_with_a_self_hosted_label_gets_the_fork_fence() -> TestResult {
    let config = build(
        "runner = \"${{ matrix.runner }}\"\nrunner_labels = { \"ubuntu-24.04\" = true, \"velnor-arm\" = false }",
        "axes = { runner = [\"ubuntu-24.04\", \"velnor-arm\"] }\n",
    );
    let runs_on = runs_on(&config)?;
    assert!(
        runs_on.contains(FENCE)
            && runs_on.contains("&& matrix.runner ||")
            && runs_on.contains("'ubuntu-latest'"),
        "{runs_on}"
    );
    Ok(())
}

#[test]
fn a_matrix_runner_label_that_is_not_declared_is_refused() -> TestResult {
    for labels in [
        "{ \"ubuntu-24.04\" = true }",
        "{ \"ubuntu-24.04\" = true, \"ubuntu-24.04-arm\" = true, \"macos-15\" = true }",
        "{}",
    ] {
        let config = build(
            &format!("runner = \"${{{{ matrix.runner }}}}\"\nrunner_labels = {labels}"),
            AXIS,
        );
        let message = refusal(&config)?;
        assert!(
            message.contains("runner_labels")
                && ["ubuntu-24.04", "ubuntu-24.04-arm", "macos-15"]
                    .iter()
                    .any(|label| message.contains(label)),
            "{labels}: {message}"
        );
    }
    Ok(())
}

#[test]
fn an_include_value_of_the_runner_axis_must_be_declared_too() -> TestResult {
    let include =
        "axes = { runner = [\"ubuntu-24.04\"] }\ninclude = [{ runner = \"velnor-box\" }]\n";
    let declared = |labels: &str| {
        build(
            &format!("runner = \"${{{{ matrix.runner }}}}\"\nrunner_labels = {labels}"),
            include,
        )
    };
    let message = refusal(&declared("{ \"ubuntu-24.04\" = true }"))?;
    assert!(message.contains("velnor-box"), "{message}");
    let both = declared("{ \"ubuntu-24.04\" = true, \"velnor-box\" = false }");
    assert!(runs_on(&both)?.contains(FENCE));
    Ok(())
}

#[test]
fn a_runner_expression_must_read_an_axis_of_the_job_matrix() -> TestResult {
    let labels = "runner_labels = { \"ubuntu-24.04\" = true }\n";
    let cases = [
        ("${{ github.actor }}", AXIS, "matrix"),
        ("${{ inputs.runner }}", AXIS, "matrix"),
        ("${{ matrix.nope }}", AXIS, "nope"),
        ("${{ matrix.runner || 'x' }}", AXIS, "matrix"),
    ];
    for (expression, matrix, needle) in cases {
        let config = build(&format!("runner = \"{expression}\"\n{labels}"), matrix);
        let message = refusal(&config)?;
        assert!(message.contains(needle), "{expression}: {message}");
    }
    Ok(())
}

#[test]
fn a_matrix_runner_cannot_read_a_matrix_built_from_a_job_output() -> TestResult {
    let config = workflow(
        "",
        ON_PULL_REQUEST,
        r#"
[[workflow.job]]
id = "plan"
outputs = { matrix = "steps.p.outputs.matrix" }

[[workflow.job.step]]
id = "p"
name = "Plan"
run = "echo ok"

[[workflow.job]]
id = "build"
needs = ["plan"]
runner = "${{ matrix.runner }}"
runner_labels = { "ubuntu-24.04" = true }

[workflow.job.matrix]
from_output = "plan.matrix"

[[workflow.job.step]]
name = "Build"
run = "echo ok"
"#,
    );
    let message = refusal(&config)?;
    assert!(message.contains("static axes"), "{message}");
    Ok(())
}

#[test]
fn matrix_runner_declarations_need_a_matrix_runner() -> TestResult {
    let single = "axes = { os = [\"a\"] }\n";
    let cases = [
        "runner = \"ubuntu-24.04\"\nrunner_labels = { \"ubuntu-24.04\" = true }",
        "runner_labels = { \"ubuntu-24.04\" = true }",
        "runner = \"${{ matrix.runner }}\"\nhosted = true\nrunner_labels = { \"ubuntu-24.04\" = true, \"ubuntu-24.04-arm\" = true }",
    ];
    for header in cases {
        let message = refusal(&build(header, AXIS))?;
        assert!(message.contains("runner_labels"), "{header}: {message}");
    }
    let message = refusal(&build("runner = \"${{ matrix.os }}\"", single))?;
    assert!(message.contains("runner_labels"), "{message}");
    Ok(())
}

#[test]
fn a_matrix_runner_job_cannot_run_a_container_on_a_macos_label() -> TestResult {
    let config = build(
        "runner = \"${{ matrix.runner }}\"\nrunner_labels = { \"ubuntu-24.04\" = true, \"macos-15\" = true }\n[workflow.job.container]\nimage = \"alpine@sha256:0000000000000000000000000000000000000000000000000000000000000000\"\n",
        "axes = { runner = [\"ubuntu-24.04\", \"macos-15\"] }\n",
    );
    let message = refusal(&config)?;
    assert!(message.contains("macos-15"), "{message}");
    Ok(())
}
