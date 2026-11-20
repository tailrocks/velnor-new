// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, job, refusal, rendered_file};
use crate::yaml_tree::Node;

/// `leaf` is a reusable workflow; `caller` runs it from a job.
pub(crate) const CONFIG: &str = r#"
[[workflow]]
file = "leaf"
name = "Leaf"

[[workflow.input]]
name = "version"

[[workflow.input]]
name = "dry_run"
type = "boolean"
default = false

[[workflow.input]]
name = "shards"
type = "number"
default = 1

[[workflow.output]]
name = "digest"
value = "build.digest"

[[workflow.job]]
id = "build"
permissions = { contents = "write" }
outputs = { digest = "steps.build.outputs.digest" }

[[workflow.job.step]]
id = "build"
name = "Build"
run = "echo ok"

[[workflow]]
file = "caller"
name = "Caller"

[workflow.on.push]
branches = ["main"]

[[workflow.job]]
id = "release"
uses = "leaf"
permissions = { contents = "write" }
with = { version = "1.2.3", dry_run = "true", shards = 4 }

[[workflow.job]]
id = "after"
needs = ["release"]

[[workflow.job.step]]
name = "Report"
run = "echo ok"
env = { DIGEST = "${{ needs.release.outputs.digest }}" }
"#;

fn caller(config: &str) -> Result<Node, Box<dyn std::error::Error>> {
    rendered_file(config, "caller")
}

#[test]
fn a_job_can_call_a_declared_reusable_workflow() -> TestResult {
    let tree = caller(CONFIG)?;
    let release = job(&tree, "release")?;
    assert_eq!(
        release.get("uses").and_then(Node::text),
        Some("./.github/workflows/leaf.yml")
    );
    assert_eq!(
        release.at(&["with", "version"]).and_then(Node::text),
        Some("1.2.3")
    );
    assert_eq!(
        release.at(&["with", "dry_run"]).and_then(Node::text),
        Some("true")
    );
    assert_eq!(
        release.at(&["with", "shards"]).and_then(Node::text),
        Some("4")
    );
    for key in ["runs-on", "steps", "timeout-minutes", "environment"] {
        assert!(release.get(key).is_none(), "{key}");
    }
    assert_eq!(
        release
            .at(&["permissions", "contents"])
            .and_then(Node::text),
        Some("write")
    );
    Ok(())
}

#[test]
fn a_later_job_reads_the_outputs_of_the_called_workflow() -> TestResult {
    let tree = caller(CONFIG)?;
    let after = job(&tree, "after")?;
    assert_eq!(after.get("needs").map(Node::texts), Some(vec!["release"]));
    let bad = CONFIG.replace("outputs.digest", "outputs.nothing");
    assert!(refusal(&bad)?.contains("outputs.nothing"));
    Ok(())
}

#[test]
fn a_call_can_use_a_local_workflow_file_or_a_pinned_remote_one() -> TestResult {
    let sha = "0123456789abcdef0123456789abcdef01234567";
    for uses in [
        "./.github/workflows/hand-written.yml".to_owned(),
        format!("tailrocks/velnor-new/.github/workflows/x.yml@{sha}"),
    ] {
        let config = CONFIG.replace("uses = \"leaf\"", &format!("uses = \"{uses}\""));
        let config = config.replace(", dry_run = \"true\", shards = 4", "");
        let config = config.replace(
            "[[workflow.job]]\nid = \"after\"\nneeds = [\"release\"]\n\n[[workflow.job.step]]\nname = \"Report\"\nrun = \"echo ok\"\nenv = { DIGEST = \"${{ needs.release.outputs.digest }}\" }\n",
            "",
        );
        let tree = caller(&config)?;
        assert_eq!(
            job(&tree, "release")?.get("uses").and_then(Node::text),
            Some(uses.as_str())
        );
    }
    Ok(())
}

#[test]
fn a_call_to_an_unknown_or_unpinned_workflow_is_refused() -> TestResult {
    let cases = [
        ("missing", "missing"),
        ("caller", "caller"),
        ("../x.yml", "../x.yml"),
        ("./.github/workflows/../x.yml", "workflows"),
        (
            "tailrocks/velnor-new/.github/workflows/x.yml@main",
            "full commit SHA",
        ),
        (
            "tailrocks/velnor-new/.github/workflows/x.yml",
            "workflow declared earlier",
        ),
    ];
    for (uses, needle) in cases {
        let config = CONFIG.replace("uses = \"leaf\"", &format!("uses = \"{uses}\""));
        let message = refusal(&config)?;
        assert!(message.contains(needle), "{uses}: {message}");
    }
    Ok(())
}

#[test]
fn a_call_must_pass_the_declared_inputs_with_the_declared_types() -> TestResult {
    let cases = [
        (
            "with = { version = \"1.2.3\", dry_run = \"true\", shards = 4 }",
            "with = { dry_run = \"true\" }",
            "version",
        ),
        (
            "version = \"1.2.3\"",
            "version = \"1.2.3\", extra = \"x\"",
            "extra",
        ),
        ("dry_run = \"true\"", "dry_run = \"maybe\"", "dry_run"),
        ("shards = 4", "shards = -4", "shards"),
        ("shards = 4", "shards = \"many\"", "shards"),
        (
            "dry_run = \"true\"",
            "dry_run = \"${{ github.sha }}\"",
            "dry_run",
        ),
        (
            "dry_run = \"true\"",
            "dry_run = \"${{ format('{0}', github.sha) }}\"",
            "dry_run",
        ),
        (
            "shards = 4",
            "shards = \"${{ github.repository }}\"",
            "shards",
        ),
        (
            "shards = 4",
            "shards = \"${{ github.sha == 'x' }}\"",
            "shards",
        ),
        (
            "shards = 4",
            "shards = \"${{ !contains(github.sha, 'x') }}\"",
            "shards",
        ),
        ("version = \"1.2.3\"", "version = 5", "version"),
        (
            "version = \"1.2.3\"",
            "version = \"${{ secrets.TOKEN }}\"",
            "secrets.TOKEN",
        ),
    ];
    for (from, to, needle) in cases {
        let config = CONFIG.replace(from, to);
        let message = refusal(&config)?;
        assert!(message.contains(needle), "{to}: {message}");
    }
    Ok(())
}

#[test]
fn a_call_cannot_grant_less_than_the_called_jobs_need_and_a_called_workflow_must_be_reusable()
-> TestResult {
    let weaker = CONFIG.replace(
        "uses = \"leaf\"\npermissions = { contents = \"write\" }",
        "uses = \"leaf\"\npermissions = { contents = \"read\" }",
    );
    assert!(refusal(&weaker)?.contains("contents"));
    let dispatch_only = r#"
[[workflow]]
file = "leaf"
name = "Leaf"

[workflow.on.workflow_dispatch]

[[workflow.job]]
id = "build"

[[workflow.job.step]]
name = "Build"
run = "echo ok"

[[workflow]]
file = "caller"
name = "Caller"

[workflow.on.push]
branches = ["main"]

[[workflow.job]]
id = "release"
uses = "leaf"
"#;
    assert!(refusal(dispatch_only)?.contains("workflow_call"));
    Ok(())
}

#[test]
fn a_call_job_refuses_the_keys_of_a_step_job() -> TestResult {
    for (extra, needle) in [
        ("runner = \"ubuntu-latest\"", "runner"),
        ("environment = \"prod\"", "environment"),
        ("timeout_minutes = 5", "timeout_minutes"),
        ("helper = true", "helper"),
        ("outputs = { x = \"steps.a.outputs.b\" }", "outputs"),
    ] {
        let config = CONFIG.replace("uses = \"leaf\"\n", &format!("uses = \"leaf\"\n{extra}\n"));
        let message = refusal(&config)?;
        assert!(message.contains(needle), "{extra}: {message}");
    }
    Ok(())
}

#[test]
fn an_expression_of_a_possible_type_is_accepted_for_a_typed_input() -> TestResult {
    for (from, to) in [
        (
            "dry_run = \"true\"",
            "dry_run = \"${{ github.sha == 'x' }}\"",
        ),
        ("version = \"1.2.3\"", "version = \"${{ github.sha }}\""),
        (
            "version = \"1.2.3\"",
            "version = \"${{ github.sha == 'x' }}\"",
        ),
    ] {
        caller(&CONFIG.replace(from, to))?;
    }
    Ok(())
}
