// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{JOB, TestResult, job, refusal, rendered, workflow};
use crate::release_fixture::steps;
use crate::yaml_tree::Node;

const ON_PUSH: &str = "[workflow.on.push]\nbranches = [\"main\"]\n";

fn job_with(extra: &str) -> String {
    workflow(
        "",
        "",
        &format!(
            "\n[[workflow.job]]\nid = \"check\"\n{extra}\n[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\n"
        ),
    )
}

#[test]
fn a_job_if_is_rendered_and_joins_the_workflow_if() -> TestResult {
    let only_job = workflow(
        "",
        "",
        &JOB.replace(
            "id = \"check\"\n",
            "id = \"check\"\nif = \"github.event_name == 'push'\"\n",
        ),
    );
    let tree = rendered(&only_job)?;
    assert_eq!(
        job(&tree, "check")?.get("if").and_then(Node::text),
        Some("${{ github.event_name == 'push' }}")
    );
    let both = workflow(
        "if = \"github.ref_name == 'main'\"",
        "",
        &JOB.replace(
            "id = \"check\"\n",
            "id = \"check\"\nif = \"github.event_name == 'push'\"\n",
        ),
    );
    let tree = rendered(&both)?;
    assert_eq!(
        job(&tree, "check")?.get("if").and_then(Node::text),
        Some("${{ (github.ref_name == 'main') && (github.event_name == 'push') }}")
    );
    Ok(())
}

#[test]
fn a_job_if_cannot_read_matrix_steps_or_a_job_that_is_not_needed() -> TestResult {
    for reference in ["matrix.os", "steps.x.outcome", "needs.other.result"] {
        let config = job_with(&format!("if = \"{reference} == 'x'\"\n"));
        let message = refusal(&config)?;
        assert!(message.contains(reference), "{message}");
    }
    Ok(())
}

#[test]
fn a_matrix_can_be_static_axes() -> TestResult {
    let config = workflow(
        "",
        "",
        r#"
[[workflow.job]]
id = "check"

[workflow.job.matrix]
fail_fast = false
max_parallel = 2

[workflow.job.matrix.axes]
os = ["ubuntu-latest", "macos-latest"]
target = ["debug", "release"]

[[workflow.job.step]]
name = "Check"
run = "echo ok"
env = { OS = "${{ matrix.os }}" }
"#,
    );
    let tree = rendered(&config)?;
    let strategy = job(&tree, "check")?.get("strategy").ok_or("no strategy")?;
    assert_eq!(
        strategy.at(&["matrix", "os"]).map(Node::texts),
        Some(vec!["ubuntu-latest", "macos-latest"])
    );
    assert_eq!(
        strategy.at(&["matrix", "target"]).map(Node::texts),
        Some(vec!["debug", "release"])
    );
    assert_eq!(
        strategy.get("fail-fast").and_then(Node::text),
        Some("false")
    );
    assert_eq!(strategy.get("max-parallel").and_then(Node::text), Some("2"));
    Ok(())
}

#[test]
fn a_static_matrix_refuses_unknown_keys_and_a_second_source() -> TestResult {
    let matrix = |body: &str| {
        workflow(
            "",
            "",
            &format!(
                "\n[[workflow.job]]\nid = \"check\"\n\n[workflow.job.matrix]\n{body}\n[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\nenv = {{ X = \"${{{{ matrix.arch }}}}\" }}\n"
            ),
        )
    };
    let cases = [
        ("fail_fast = false".to_owned(), "exactly one"),
        (
            "from_output = \"x.y\"\naxes = { os = [\"a\"] }".to_owned(),
            "exactly one",
        ),
        ("axes = { os = [\"a\"] }".to_owned(), "matrix.arch"),
        (
            "axes = { include = [\"a\"], arch = [\"a\"] }".to_owned(),
            "include",
        ),
        (
            "axes = { arch = [\"a\"], ARCH = [\"b\"] }".to_owned(),
            "only by case",
        ),
        ("axes = { arch = [] }".to_owned(), "arch"),
        ("axes = { arch = [\"a\", \"a\"] }".to_owned(), "arch"),
        (
            "axes = { arch = [\"${{ github.sha }}\"] }".to_owned(),
            "arch",
        ),
    ];
    for (body, needle) in cases {
        let message = refusal(&matrix(&body))?;
        assert!(message.contains(needle), "{body}: {message}");
    }
    Ok(())
}

#[test]
fn a_hosted_runner_has_no_fork_fence_and_a_self_hosted_one_has() -> TestResult {
    let hosted = rendered(&workflow(
        "runner = \"ubuntu-24.04\"\nhosted = true",
        ON_PUSH,
        JOB,
    ))?;
    assert_eq!(
        job(&hosted, "check")?.get("runs-on").and_then(Node::text),
        Some("ubuntu-24.04")
    );
    let fenced = rendered(&workflow(
        "runner = \"velnor-linux\"\nfork_runner = \"ubuntu-24.04\"",
        "[workflow.on.pull_request]\ntypes = [\"opened\"]\n",
        JOB,
    ))?;
    let runs_on = job(&fenced, "check")?
        .get("runs-on")
        .and_then(Node::text)
        .ok_or("no runs-on")?;
    assert!(runs_on.contains("github.event.pull_request.head.repo.full_name == github.repository"));
    assert!(runs_on.contains("'velnor-linux'") && runs_on.contains("'ubuntu-24.04'"));
    Ok(())
}

#[test]
fn a_job_can_pick_its_own_runner() -> TestResult {
    let config = workflow(
        "",
        ON_PUSH,
        &JOB.replace(
            "id = \"check\"\n",
            "id = \"check\"\nrunner = \"macos-15\"\nhosted = true\n",
        ),
    );
    let tree = rendered(&config)?;
    assert_eq!(
        job(&tree, "check")?.get("runs-on").and_then(Node::text),
        Some("macos-15")
    );
    Ok(())
}

#[test]
fn a_runner_label_cannot_carry_an_expression_and_the_fork_runner_must_be_hosted() -> TestResult {
    for (header, needle) in [
        ("runner = \"${{ github.actor }}\"", "runner"),
        ("runner = \"a b\"", "runner"),
        ("fork_runner = \"my-self-hosted\"", "GitHub-hosted"),
    ] {
        let message = refusal(&workflow(header, "", JOB))?;
        assert!(message.contains(needle), "{header}: {message}");
    }
    Ok(())
}

#[test]
fn permissions_are_empty_unless_a_job_names_them() -> TestResult {
    let tree = rendered(&workflow("", "", JOB))?;
    assert_eq!(
        tree.get("permissions").map(|node| node.keys().len()),
        Some(0)
    );
    assert_eq!(
        job(&tree, "check")?
            .get("permissions")
            .map(|node| node.keys().len()),
        Some(0)
    );
    let scoped = rendered(&job_with("permissions = { contents = \"read\" }\n"))?;
    assert_eq!(
        job(&scoped, "check")?
            .at(&["permissions", "contents"])
            .and_then(Node::text),
        Some("read")
    );
    Ok(())
}

#[test]
fn timeout_minutes_default_to_thirty_and_stay_in_range() -> TestResult {
    let tree = rendered(&workflow("", "", JOB))?;
    assert_eq!(
        job(&tree, "check")?
            .get("timeout-minutes")
            .and_then(Node::text),
        Some("30")
    );
    let set = rendered(&job_with("timeout_minutes = 7\n"))?;
    assert_eq!(
        job(&set, "check")?
            .get("timeout-minutes")
            .and_then(Node::text),
        Some("7")
    );
    for bad in ["0", "361"] {
        let message = refusal(&job_with(&format!("timeout_minutes = {bad}\n")))?;
        assert!(message.contains("timeout_minutes"), "{message}");
    }
    Ok(())
}

const ATTEST: &str = r#"
[[workflow.job]]
id = "attest"
permissions = { id-token = "write", attestations = "write", contents = "read" }

[[workflow.job.step]]
uses = "attest"
with = { subject-path = "dist/app.tar.gz" }
"#;

#[test]
fn the_attest_action_is_pinned_and_never_pushes_to_a_registry() -> TestResult {
    let tree = rendered(&workflow("", "", ATTEST))?;
    let attest = job(&tree, "attest")?;
    let step = steps(attest).first().ok_or("no step")?;
    let uses = step.get("uses").and_then(Node::text).ok_or("no uses")?;
    assert!(uses.starts_with("actions/attest-build-provenance@") && uses.len() == 72);
    assert_eq!(
        step.at(&["with", "push-to-registry"]).and_then(Node::text),
        Some("false")
    );
    assert_eq!(
        step.at(&["with", "subject-path"]).and_then(Node::text),
        Some("dist/app.tar.gz")
    );
    Ok(())
}

#[test]
fn the_attest_action_needs_contents_read() -> TestResult {
    for permissions in [
        "permissions = { id-token = \"write\", attestations = \"write\" }",
        "permissions = { id-token = \"write\", attestations = \"write\", contents = \"none\" }",
    ] {
        let config = ATTEST.replace(
            "permissions = { id-token = \"write\", attestations = \"write\", contents = \"read\" }",
            permissions,
        );
        let message = refusal(&workflow("", "", &config))?;
        assert!(message.contains("contents"), "{permissions}: {message}");
    }
    Ok(())
}

#[test]
fn the_attest_action_needs_its_permissions_and_keeps_its_lock() -> TestResult {
    let without = ATTEST.replace("id-token = \"write\", ", "");
    let message = refusal(&workflow("", "", &without))?;
    assert!(message.contains("id-token"), "{message}");
    let pushing = ATTEST.replace("with = { ", "with = { push-to-registry = \"true\", ");
    let message = refusal(&workflow("", "", &pushing))?;
    assert!(message.contains("push-to-registry"), "{message}");
    Ok(())
}

fn needs_condition(condition: &str) -> String {
    workflow(
        "",
        "",
        &format!(
            "\n[[workflow.job]]\nid = \"first\"\n\n[[workflow.job.step]]\nname = \"First\"\nrun = \"echo ok\"\n\n[[workflow.job]]\nid = \"second\"\nneeds = [\"first\"]\nif = \"{condition}\"\n\n[[workflow.job.step]]\nname = \"Second\"\nrun = \"echo ok\"\n"
        ),
    )
}

#[test]
fn a_wildcard_needs_reference_must_name_a_real_property() -> TestResult {
    for condition in [
        "always() && !contains(needs.*.reslt, 'failure')",
        "needs.*.anything == 'x'",
        "needs.*.result.extra == 'x'",
        "needs.* == 'x'",
    ] {
        let message = refusal(&needs_condition(condition))?;
        assert!(message.contains("needs.*"), "{condition}: {message}");
    }
    rendered(&needs_condition(
        "always() && !contains(needs.*.result, 'failure')",
    ))?;
    Ok(())
}

#[test]
fn a_matrix_job_cannot_declare_outputs() -> TestResult {
    let config = |outputs: &str| {
        workflow(
            "",
            "",
            &format!(
                "\n[[workflow.job]]\nid = \"build\"\n{outputs}\n[workflow.job.matrix]\naxes = {{ os = [\"a\", \"b\"] }}\n\n[[workflow.job.step]]\nid = \"build\"\nname = \"Build\"\nrun = \"echo ok\"\n"
            ),
        )
    };
    let message = refusal(&config(
        "outputs = { digest = \"steps.build.outputs.digest\" }",
    ))?;
    assert!(
        message.contains("matrix") && message.contains("outputs"),
        "{message}"
    );
    rendered(&config(""))?;
    Ok(())
}
