// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{JOB, TestResult, job, refusal, rendered, workflow};
use crate::yaml_tree::Node;

const PUSH: &str = "[workflow.on.push]\nbranches = [\"main\"]\n";
const GROUP: &str = "ci-${{ github.workflow }}-${{ github.head_ref || github.run_id }}";

fn with_concurrency(table: &str) -> String {
    workflow(&format!("[workflow.concurrency]\n{table}\n"), PUSH, JOB)
}

#[test]
fn a_workflow_concurrency_renders_between_permissions_and_jobs() -> TestResult {
    let tree = rendered(&with_concurrency(&format!(
        "group = \"{GROUP}\"\ncancel_in_progress = true"
    )))?;
    assert_eq!(
        tree.at(&["concurrency", "group"]).and_then(Node::text),
        Some(GROUP)
    );
    assert_eq!(
        tree.at(&["concurrency", "cancel-in-progress"])
            .and_then(Node::text),
        Some("true")
    );
    assert_eq!(
        tree.keys(),
        ["name", "on", "permissions", "concurrency", "jobs"]
    );
    Ok(())
}

#[test]
fn without_a_concurrency_the_file_has_none_and_cancelling_defaults_to_off() -> TestResult {
    let plain = rendered(&workflow("", PUSH, JOB))?;
    assert!(plain.get("concurrency").is_none());
    assert!(!plain.has_key_anywhere("concurrency"));
    let tree = rendered(&with_concurrency(
        "group = \"nightly-${{ github.workflow }}\"",
    ))?;
    assert_eq!(
        tree.at(&["concurrency", "cancel-in-progress"])
            .and_then(Node::text),
        Some("false")
    );
    Ok(())
}

#[test]
fn a_job_concurrency_renders_on_a_step_job_and_on_a_call_job() -> TestResult {
    let group = "build-${{ github.workflow }}-${{ github.ref }}";
    let step_job = workflow(
        "",
        PUSH,
        &format!(
            "\n[[workflow.job]]\nid = \"check\"\nconcurrency = {{ group = \"{group}\", cancel_in_progress = true }}\n\n[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\n"
        ),
    );
    let tree = rendered(&step_job)?;
    let check = job(&tree, "check")?;
    assert_eq!(
        check.at(&["concurrency", "group"]).and_then(Node::text),
        Some(group)
    );
    assert_eq!(
        check
            .at(&["concurrency", "cancel-in-progress"])
            .and_then(Node::text),
        Some("true")
    );
    assert!(tree.get("concurrency").is_none());
    let call = "[[workflow]]\nfile = \"leaf\"\nname = \"Leaf\"\n\n[[workflow.job]]\nid = \"build\"\n\n[[workflow.job.step]]\nname = \"Build\"\nrun = \"echo ok\"\n\n[[workflow]]\nfile = \"ci\"\nname = \"CI\"\n\n[workflow.on.push]\nbranches = [\"main\"]\n\n[[workflow.job]]\nid = \"release\"\nuses = \"leaf\"\nconcurrency = { group = \"release-${{ github.ref }}\" }\n";
    let tree = rendered(call)?;
    let release = job(&tree, "release")?;
    assert_eq!(
        release.at(&["concurrency", "group"]).and_then(Node::text),
        Some("release-${{ github.ref }}")
    );
    assert_eq!(
        release.get("uses").and_then(Node::text),
        Some("./.github/workflows/leaf.yml")
    );
    Ok(())
}

#[test]
fn every_allowed_context_and_a_plain_literal_are_accepted_as_a_group() -> TestResult {
    for group in [
        "nightly",
        "ci-${{ github.workflow }}",
        "${{github.workflow}}-${{ github.ref }}",
        "pr-${{ github.event.pull_request.number }}-${{ github.head_ref || github.run_id }}",
        "a.b_c:d/e-${{ github.head_ref   ||   github.run_id }}",
    ] {
        rendered(&with_concurrency(&format!("group = \"{group}\"")))?;
    }
    Ok(())
}

#[test]
fn a_group_that_reads_anything_else_or_is_malformed_is_refused() -> TestResult {
    let cases = [
        ("${{ secrets.TOKEN }}", "secrets.TOKEN"),
        (
            "x-${{ github.event.pull_request.title }}",
            "pull_request.title",
        ),
        ("x-${{ github.actor }}", "github.actor"),
        ("x-${{ inputs.mode }}", "inputs.mode"),
        ("x-${{ github.ref && github.sha }}", "github.sha"),
        ("x-${{ format('{0}', github.ref) }}", "format"),
        ("x-${{ github.ref", "unterminated"),
        ("x-${{ }}", "empty"),
        ("x }} y", "group"),
        ("has space", "group"),
        ("line\\nbreak", "group"),
        ("", "group"),
        ("x-$HOME", "group"),
        ("x-${{ github.ref }}${{ github.workflow", "unterminated"),
    ];
    for (group, needle) in cases {
        let message = refusal(&with_concurrency(&format!("group = \"{group}\"")))?;
        assert!(message.contains(needle), "{group}: {message}");
    }
    let job_level = workflow(
        "",
        PUSH,
        "\n[[workflow.job]]\nid = \"check\"\nconcurrency = { group = \"x-${{ github.actor }}\" }\n\n[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\n",
    );
    assert!(refusal(&job_level)?.contains("github.actor"));
    Ok(())
}

#[test]
fn cancelling_needs_a_group_that_tells_refs_apart_and_unknown_keys_are_refused() -> TestResult {
    let cases = [
        (
            "group = \"ci-${{ github.workflow }}\"\ncancel_in_progress = true",
            "github.ref",
        ),
        ("group = \"ci\"\ncancel_in_progress = true", "github.ref"),
        (
            "group = \"ci-${{ github.event.pull_request.number }}\"\ncancel_in_progress = true",
            "github.ref",
        ),
        (
            "group = \"ci-${{ github.ref }}\"\ncancel-in-progress = true",
            "cancel-in-progress",
        ),
        ("cancel_in_progress = true", "group"),
    ];
    for (table, needle) in cases {
        let message = refusal(&with_concurrency(table))?;
        assert!(message.contains(needle), "{table}: {message}");
    }
    rendered(&with_concurrency(
        "group = \"ci-${{ github.ref }}\"\ncancel_in_progress = true",
    ))?;
    Ok(())
}
