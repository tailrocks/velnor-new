// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, job, refusal, rendered, workflow};
use crate::release_fixture::steps;
use crate::yaml_tree::Node;

const STEPS: &str = r#"
[[workflow.job]]
id = "check"
permissions = { contents = "read" }

[[workflow.job.step]]
uses = "checkout"
with = { WITH }

[[workflow.job.step]]
uses = "mise"
with = { MISE }

[[workflow.job.step]]
name = "Run"
run = "RUN"
env = { ENV }
"#;

fn config(with: &str, mise: &str, run: &str, env: &str) -> String {
    workflow(
        "",
        "",
        &STEPS
            .replace("WITH", with)
            .replace("MISE", mise)
            .replace("RUN", run)
            .replace("ENV", env),
    )
}

fn plain() -> String {
    config(
        "fetch-depth = \"1\"",
        "version = \"2026.11.9\"",
        "echo ok",
        "A = \"b\"",
    )
}

#[test]
fn a_run_step_is_one_simple_command() -> TestResult {
    for (run, needle) in [
        ("echo a\\necho b", "more than one line"),
        ("echo a; echo b", ";"),
        ("echo a | cat", "|"),
        ("echo a & echo b", "&"),
        ("echo a && echo b", "&"),
        ("echo $(id)", "$("),
        ("echo `id`", "`"),
        ("echo ${{ github.sha }}", "expression"),
        ("if true then echo a", "if"),
    ] {
        let message = refusal(&config(
            "fetch-depth = \"1\"",
            "version = \"1\"",
            run,
            "A = \"b\"",
        ))?;
        assert!(message.contains(needle), "{run}: {message}");
    }
    Ok(())
}

#[test]
fn an_expression_reaches_a_command_only_through_env() -> TestResult {
    let tree = rendered(&config(
        "fetch-depth = \"1\"",
        "version = \"1\"",
        "echo \\\"$SHA\\\"",
        "SHA = \"${{ github.sha }}\"",
    ))?;
    let run = steps(job(&tree, "check")?)
        .iter()
        .find(|step| step.get("name").and_then(Node::text) == Some("Run"))
        .ok_or("no run step")?;
    assert_eq!(
        run.at(&["env", "SHA"]).and_then(Node::text),
        Some("${{ github.sha }}")
    );
    assert!(
        !run.get("run")
            .and_then(Node::text)
            .ok_or("no run")?
            .contains("${{")
    );
    Ok(())
}

#[test]
fn an_env_value_is_plain_text_or_exactly_one_expression() -> TestResult {
    for env in [
        "A = \"x ${{ github.sha }}\"",
        "A = \"${{ github.sha }}${{ github.token }}\"",
        "A = \"${{ secrets.TOKEN }}\"",
        "A = \"${{ github.event.pull_request.title }}\"",
    ] {
        refusal(&config(
            "fetch-depth = \"1\"",
            "version = \"1\"",
            "echo ok",
            env,
        ))?;
    }
    Ok(())
}

#[test]
fn locked_inputs_stay_off_and_an_override_is_refused() -> TestResult {
    let tree = rendered(&plain())?;
    let all = steps(job(&tree, "check")?);
    assert_eq!(
        all[0]
            .at(&["with", "persist-credentials"])
            .and_then(Node::text),
        Some("false")
    );
    assert_eq!(
        all[1].at(&["with", "cache"]).and_then(Node::text),
        Some("false")
    );
    assert_eq!(
        all[1].at(&["with", "cache_save"]).and_then(Node::text),
        Some("false")
    );
    for (with, mise) in [
        ("persist-credentials = \"true\"", "version = \"1\""),
        ("fetch-depth = \"1\"", "cache = \"true\""),
        ("fetch-depth = \"1\"", "cache_save = \"true\""),
    ] {
        let message = refusal(&config(with, mise, "echo ok", "A = \"b\""))?;
        assert!(message.contains("always false"), "{with} {mise}: {message}");
    }
    Ok(())
}

#[test]
fn only_the_pinned_action_list_is_available() -> TestResult {
    for action in [
        "actions/checkout",
        "evil/action@v1",
        "docker://alpine",
        "./local",
    ] {
        let config = workflow(
            "",
            "",
            &format!(
                "\n[[workflow.job]]\nid = \"check\"\n\n[[workflow.job.step]]\nuses = \"{action}\"\n"
            ),
        );
        let message = refusal(&config)?;
        assert!(message.contains("pinned actions"), "{action}: {message}");
    }
    Ok(())
}

#[test]
fn every_rendered_action_is_pinned_to_a_full_sha() -> TestResult {
    let tree = rendered(&plain())?;
    for step in steps(job(&tree, "check")?) {
        if let Some(uses) = step.get("uses").and_then(Node::text) {
            let (_, sha) = uses.split_once('@').ok_or("unpinned")?;
            assert_eq!(sha.len(), 40, "{uses}");
        }
    }
    Ok(())
}

#[test]
fn a_run_step_that_calls_the_helper_needs_helper_installed() -> TestResult {
    let message = refusal(&config(
        "fetch-depth = \"1\"",
        "version = \"1\"",
        "velnor-ci check",
        "A = \"b\"",
    ))?;
    assert!(message.contains("helper = true"), "{message}");
    Ok(())
}
