// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, job, refusal, rendered, workflow};
use crate::yaml_tree::Node;

const RUN: &str = "[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\n";

fn one_job(header: &str, job_keys: &str, steps: &str) -> String {
    workflow(
        header,
        "",
        &format!("\n[[workflow.job]]\nid = \"check\"\n{job_keys}\n{steps}"),
    )
}

fn input(name: &str) -> String {
    format!("\n[[workflow.input]]\nname = \"{name}\"\n")
}

fn dispatch(names: impl Iterator<Item = String>) -> String {
    let inputs: String = names
        .map(|name| format!("\n[[workflow.on.workflow_dispatch.input]]\nname = \"{name}\"\n"))
        .collect();
    workflow(
        "",
        &format!("\n[workflow.on.workflow_dispatch]\n{inputs}"),
        &format!("\n[[workflow.job]]\nid = \"check\"\n{RUN}"),
    )
}

#[test]
fn an_environment_name_is_at_most_255_characters() -> TestResult {
    let long = "e".repeat(256);
    let message = refusal(&one_job("", &format!("environment = \"{long}\""), RUN))?;
    assert!(
        message.contains("255") && message.contains("job check"),
        "{message}"
    );
    let fits = "e".repeat(255);
    let tree = rendered(&one_job("", &format!("environment = \"{fits}\""), RUN))?;
    assert_eq!(
        job(&tree, "check")?.get("environment").and_then(Node::text),
        Some(fits.as_str())
    );
    Ok(())
}

#[test]
fn input_names_are_unique_ignoring_case() -> TestResult {
    let call = format!("{}{}", input("sdk_version"), input("SDK_VERSION"));
    let message = refusal(&one_job(&call, "", RUN))?;
    assert!(message.contains("SDK_VERSION"), "{message}");
    let message = refusal(&dispatch(["mode", "MODE"].map(str::to_owned).into_iter()))?;
    assert!(message.contains("MODE"), "{message}");
    rendered(&one_job(
        &format!("{}{}", input("sdk_version"), input("sdk_other")),
        "",
        RUN,
    ))?;
    Ok(())
}

#[test]
fn a_dispatch_has_at_most_25_inputs() -> TestResult {
    rendered(&dispatch((0..25).map(|index| format!("input{index}"))))?;
    let message = refusal(&dispatch((0..26).map(|index| format!("input{index}"))))?;
    assert!(
        message.contains("25") && message.contains("workflow_dispatch"),
        "{message}"
    );
    Ok(())
}

#[test]
fn a_write_token_is_not_given_to_a_repository_command() -> TestResult {
    let publish = "[[workflow.job.step]]\nname = \"Publish\"\nrun = \"bash scripts/publish.sh\"\nenv = { GH_TOKEN = \"${{ github.token }}\" }\n";
    let write = "permissions = { contents = \"write\" }";
    let message = refusal(&one_job("", write, publish))?;
    assert!(
        message.contains("github.token") && message.contains("job check"),
        "{message}"
    );
    rendered(&one_job(
        "",
        "permissions = { contents = \"read\" }",
        publish,
    ))?;
    rendered(&one_job("", write, RUN))?;
    let helper = "[[workflow.job.step]]\nname = \"Plan\"\nrun = \"velnor-ci plan\"\nenv = { GH_TOKEN = \"${{ github.token }}\" }\n";
    rendered(&one_job("", &format!("helper = true\n{write}"), helper))?;
    Ok(())
}

#[test]
fn a_cache_key_cannot_be_an_unbounded_expression() -> TestResult {
    for uses in ["cache", "cache-save", "cache-restore"] {
        let step = |key: &str| {
            format!(
                "[[workflow.job.step]]\nname = \"Step\"\nuses = \"{uses}\"\nwith = {{ path = \"a\", key = \"{key}\" }}\n"
            )
        };
        let config = |key: &str| one_job(&input("cache_key"), "", &step(key));
        let message = refusal(&config("${{ inputs.cache_key }}"))?;
        assert!(message.contains("cache key"), "{uses}: {message}");
        rendered(&config("fixed-${{ github.run_attempt }}"))?;
    }
    Ok(())
}

#[test]
fn functions_are_checked_in_the_scope_they_are_evaluated_in() -> TestResult {
    let hashed = "hashFiles('Cargo.lock') != ''";
    let message = refusal(&one_job("", &format!("if = \"{hashed}\""), RUN))?;
    assert!(message.contains("hashFiles"), "{message}");
    let step =
        format!("[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\nif = \"{hashed}\"\n");
    rendered(&one_job("", "", &step))?;
    let message = refusal(&one_job("", "if = \"nonsense(1) == 1\"", RUN))?;
    assert!(message.contains("nonsense"), "{message}");
    let env = "[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\nenv = { X = \"${{ failure() }}\" }\n";
    let message = refusal(&one_job("", "", env))?;
    assert!(message.contains("failure"), "{message}");
    rendered(&one_job(
        "",
        "if = \"always() && contains(github.ref, 'x') && startsWith(github.ref, 'r')\"",
        RUN,
    ))?;
    Ok(())
}

#[test]
fn a_matrix_built_from_an_output_is_skipped_while_the_output_is_empty() -> TestResult {
    let plan = "\n[[workflow.job]]\nid = \"plan\"\nhelper = true\noutputs = { matrix = \"steps.plan.outputs.matrix\" }\n\n[[workflow.job.step]]\nid = \"plan\"\nname = \"Plan\"\nrun = \"velnor-ci plan\"\n";
    let check = |extra: &str| {
        format!(
            "{plan}\n[[workflow.job]]\nid = \"check\"\nneeds = [\"plan\"]\n{extra}\n[workflow.job.matrix]\nfrom_output = \"plan.matrix\"\n\n{RUN}"
        )
    };
    let tree = rendered(&workflow("", "", &check("")))?;
    assert_eq!(
        job(&tree, "check")?.get("if").and_then(Node::text),
        Some("${{ needs.plan.outputs.matrix != '' }}")
    );
    let tree = rendered(&workflow("", "", &check("if = \"github.ref == 'x'\"")))?;
    assert_eq!(
        job(&tree, "check")?.get("if").and_then(Node::text),
        Some("${{ (needs.plan.outputs.matrix != '') && (github.ref == 'x') }}")
    );
    Ok(())
}
