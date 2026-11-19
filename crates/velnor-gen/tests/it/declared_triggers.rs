// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{JOB, TestResult, refusal, rendered, workflow};
use crate::yaml_tree::Node;

fn with_on(on: &str) -> String {
    workflow("", on, JOB)
}

fn on_keys(on: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let tree = rendered(&with_on(on))?;
    Ok(tree
        .get("on")
        .ok_or("no on")?
        .keys()
        .into_iter()
        .map(str::to_owned)
        .collect())
}

#[test]
fn without_an_on_table_the_workflow_is_reusable() -> TestResult {
    assert_eq!(on_keys("")?, ["workflow_call"]);
    Ok(())
}

#[test]
fn workflow_call_declares_typed_inputs_and_outputs() -> TestResult {
    let config = workflow(
        "",
        r#"
[workflow.on]
workflow_call = true

[[workflow.input]]
name = "version"
description = "Version to build"
type = "string"

[[workflow.input]]
name = "dry_run"
type = "boolean"
default = false

[[workflow.input]]
name = "shards"
type = "number"
default = 3

[[workflow.output]]
name = "artifact"
description = "The built artifact"
value = "check.artifact"
"#,
        r#"
[[workflow.job]]
id = "check"
outputs = { artifact = "steps.build.outputs.id" }

[[workflow.job.step]]
id = "build"
name = "Build"
run = "echo ok"
"#,
    );
    let tree = rendered(&config)?;
    let call = tree.at(&["on", "workflow_call"]).ok_or("no call")?;
    let input = |name: &str, key: &str| {
        call.at(&["inputs", name, key])
            .and_then(Node::text)
            .map(str::to_owned)
    };
    assert_eq!(input("version", "type").as_deref(), Some("string"));
    assert_eq!(input("version", "required").as_deref(), Some("true"));
    assert_eq!(input("dry_run", "type").as_deref(), Some("boolean"));
    assert_eq!(input("dry_run", "default").as_deref(), Some("false"));
    assert_eq!(input("dry_run", "required").as_deref(), Some("false"));
    assert_eq!(input("shards", "type").as_deref(), Some("number"));
    assert_eq!(input("shards", "default").as_deref(), Some("3"));
    assert_eq!(
        call.at(&["outputs", "artifact", "value"])
            .and_then(Node::text),
        Some("${{ jobs.check.outputs.artifact }}")
    );
    Ok(())
}

#[test]
fn workflow_dispatch_declares_inputs_with_choices() -> TestResult {
    let tree = rendered(&with_on(
        r#"
[workflow.on.workflow_dispatch]

[[workflow.on.workflow_dispatch.input]]
name = "mode"
description = "How much to run"
type = "choice"
options = ["quick", "full"]
default = "quick"

[[workflow.on.workflow_dispatch.input]]
name = "environment"
type = "environment"
"#,
    ))?;
    let inputs = tree
        .at(&["on", "workflow_dispatch", "inputs"])
        .ok_or("no dispatch inputs")?;
    assert_eq!(
        inputs.at(&["mode", "type"]).and_then(Node::text),
        Some("choice")
    );
    assert_eq!(
        inputs.at(&["mode", "options"]).map(Node::texts),
        Some(vec!["quick", "full"])
    );
    assert_eq!(
        inputs.at(&["mode", "default"]).and_then(Node::text),
        Some("quick")
    );
    assert_eq!(
        inputs.at(&["environment", "type"]).and_then(Node::text),
        Some("environment")
    );
    assert_eq!(
        on_keys("[workflow.on.workflow_dispatch]")?,
        ["workflow_dispatch"]
    );
    Ok(())
}

#[test]
fn schedule_lists_cron_expressions() -> TestResult {
    let tree = rendered(&with_on(
        "[workflow.on]\nschedule = [\"23 4 * * 1\", \"0 6 * * *\"]\n",
    ))?;
    let schedule = tree.at(&["on", "schedule"]).ok_or("no schedule")?;
    let crons: Vec<_> = schedule
        .items()
        .iter()
        .filter_map(|item| item.get("cron").and_then(Node::text))
        .collect();
    assert_eq!(crons, ["23 4 * * 1", "0 6 * * *"]);
    Ok(())
}

#[test]
fn push_filters_branches_and_tags_and_the_tag_prefix_is_config() -> TestResult {
    for tag in ["rust-ci-v*", "v*", "sdk-preview-*"] {
        let tree = rendered(&with_on(&format!(
            "[workflow.on.push]\nbranches = [\"main\"]\ntags = [\"{tag}\"]\n"
        )))?;
        let push = tree.at(&["on", "push"]).ok_or("no push")?;
        assert_eq!(push.get("branches").map(Node::texts), Some(vec!["main"]));
        assert_eq!(push.get("tags").map(Node::texts), Some(vec![tag]));
    }
    let tags_only = rendered(&with_on("[workflow.on.push]\ntags = [\"v*\"]\n"))?;
    assert!(tags_only.at(&["on", "push", "branches"]).is_none());
    Ok(())
}

#[test]
fn pull_request_lists_types_and_branches() -> TestResult {
    let tree = rendered(&with_on(
        "[workflow.on.pull_request]\ntypes = [\"opened\", \"synchronize\", \"ready_for_review\"]\nbranches = [\"main\"]\n",
    ))?;
    let request = tree.at(&["on", "pull_request"]).ok_or("no pull_request")?;
    assert_eq!(
        request.get("types").map(Node::texts),
        Some(vec!["opened", "synchronize", "ready_for_review"])
    );
    assert_eq!(request.get("branches").map(Node::texts), Some(vec!["main"]));
    Ok(())
}

#[test]
fn merge_group_is_a_plain_trigger() -> TestResult {
    assert_eq!(
        on_keys("[workflow.on]\nmerge_group = true\n")?,
        ["merge_group"]
    );
    Ok(())
}

#[test]
fn workflow_run_follows_named_workflows_and_defaults_to_completed() -> TestResult {
    let tree = rendered(&with_on(
        "[workflow.on.workflow_run]\nworkflows = [\"Rust CI\"]\nbranches = [\"main\"]\n",
    ))?;
    let run = tree.at(&["on", "workflow_run"]).ok_or("no workflow_run")?;
    assert_eq!(run.get("workflows").map(Node::texts), Some(vec!["Rust CI"]));
    assert_eq!(run.get("types").map(Node::texts), Some(vec!["completed"]));
    assert_eq!(run.get("branches").map(Node::texts), Some(vec!["main"]));
    Ok(())
}

#[test]
fn events_come_out_in_one_fixed_order_whatever_the_config_order() -> TestResult {
    let every = r#"
[workflow.on]
merge_group = true
schedule = ["0 6 * * 1"]
workflow_call = true

[workflow.on.workflow_run]
workflows = ["Rust CI"]

[workflow.on.pull_request]
types = ["opened"]

[workflow.on.push]
branches = ["main"]

[workflow.on.workflow_dispatch]
"#;
    assert_eq!(
        on_keys(every)?,
        [
            "workflow_call",
            "workflow_dispatch",
            "schedule",
            "push",
            "pull_request",
            "merge_group",
            "workflow_run"
        ]
    );
    Ok(())
}

#[test]
fn a_dispatch_input_can_be_read_in_an_expression() -> TestResult {
    let config = workflow(
        "",
        "[workflow.on.workflow_dispatch]\n\n[[workflow.on.workflow_dispatch.input]]\nname = \"mode\"\ndefault = \"quick\"\n",
        r#"
[[workflow.job]]
id = "check"

[[workflow.job.step]]
name = "Check"
run = "echo ok"
env = { MODE = "${{ inputs.mode }}" }
"#,
    );
    rendered(&config)?;
    Ok(())
}

#[test]
fn a_trigger_that_is_unsafe_or_malformed_is_refused() -> TestResult {
    let cases = [
        ("[workflow.on.push]\n", "branches or tags"),
        ("[workflow.on.pull_request]\n", "types"),
        (
            "[workflow.on.pull_request]\ntypes = [\"nonsense\"]\n",
            "nonsense",
        ),
        ("[workflow.on]\nschedule = [\"every day\"]\n", "every day"),
        ("[workflow.on]\nschedule = [\"* * * *\"]\n", "five-field"),
        ("[workflow.on.workflow_run]\n", "workflows"),
        (
            "[workflow.on.workflow_run]\nworkflows = [\"CI\"]\ntypes = [\"nope\"]\n",
            "nope",
        ),
        (
            "[workflow.on]\nmerge_group = false\n",
            "at least one trigger",
        ),
        (
            "[workflow.on.push]\ntags = [\"v* ${{ github.sha }}\"]\n",
            "pattern",
        ),
        (
            "[workflow.on]\npull_request_target = true\n",
            "pull_request_target",
        ),
        (
            "[workflow.on]\nmerge_group = true\n\n[[workflow.input]]\nname = \"x\"\n",
            "workflow_call",
        ),
    ];
    for (on, needle) in cases {
        let message = refusal(&with_on(on))?;
        assert!(message.contains(needle), "{on}: {message}");
    }
    Ok(())
}

#[test]
fn a_malformed_input_is_refused() -> TestResult {
    let dispatch = |input: &str| {
        with_on(&format!(
            "[workflow.on.workflow_dispatch]\n\n[[workflow.on.workflow_dispatch.input]]\nname = \"x\"\n{input}\n"
        ))
    };
    let cases = [
        ("type = \"choice\"", "choice"),
        (
            "type = \"choice\"\noptions = [\"a\"]\ndefault = \"b\"",
            "default",
        ),
        ("type = \"boolean\"\ndefault = \"yes\"", "boolean"),
        ("type = \"number\"\ndefault = -1", "number"),
        ("type = \"integer\"", "integer"),
        ("options = [\"a\"]", "options"),
    ];
    for (input, needle) in cases {
        let message = refusal(&dispatch(input))?;
        assert!(message.contains(needle), "{input}: {message}");
    }
    Ok(())
}
