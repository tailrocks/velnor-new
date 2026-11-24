// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, job, refusal, rendered, workflow};
use crate::release_fixture::steps;
use crate::yaml_tree::Node;

const DISPATCH: &str = "\n[workflow.on.workflow_dispatch]\n";

/// One job `hold` whose step has `step` as extra keys, in a workflow with `on`.
fn config(on: &str, job: &str, step: &str) -> String {
    workflow(
        "",
        on,
        &format!(
            "\n[[workflow.job]]\nid = \"hold\"\n{job}\n[[workflow.job.step]]\nname = \"Hold\"\nrun = \"sleep 1\"\n{step}\n"
        ),
    )
}

fn secret_env(expression: &str) -> String {
    format!("env = {{ CANARY = \"{expression}\" }}")
}

fn canary() -> String {
    secret_env("${{ secrets.G3_CANARY }}")
}

#[test]
fn a_secret_is_allowed_as_a_step_env_value_in_a_dispatch_only_workflow() -> TestResult {
    let tree = rendered(&config(DISPATCH, "", &canary()))?;
    let hold = steps(job(&tree, "hold")?);
    assert_eq!(
        hold[0].at(&["env", "CANARY"]).and_then(Node::text),
        Some("${{ secrets.G3_CANARY }}")
    );
    assert_eq!(hold[0].get("run").and_then(Node::text), Some("sleep 1"));
    Ok(())
}

#[test]
fn a_secret_is_refused_in_step_with_run_and_if() -> TestResult {
    let step_with = "[workflow.job.step.with]\ntoken = \"${{ secrets.G3_CANARY }}\"\n";
    let checkout = format!(
        "\n[[workflow.job]]\nid = \"hold\"\n\n[[workflow.job.step]]\nuses = \"checkout\"\n{step_with}"
    );
    let message = refusal(&workflow("", DISPATCH, &checkout))?;
    assert!(message.contains("secrets.G3_CANARY"), "with: {message}");
    assert!(message.contains("step env"), "with: {message}");
    // `run` never holds an expression, a secret included.
    let run = config(DISPATCH, "", "").replace("sleep 1", "echo ${{ secrets.G3_CANARY }}");
    let message = refusal(&run)?;
    assert!(message.contains("expression"), "run: {message}");
    let message = refusal(&config(DISPATCH, "", "if = \"secrets.G3_CANARY != ''\""))?;
    assert!(message.contains("secrets.G3_CANARY"), "if: {message}");
    assert!(message.contains("step env"), "if: {message}");
    Ok(())
}

#[test]
fn a_secret_is_refused_in_a_job_env_and_in_a_container_or_service_env() -> TestResult {
    let job_env = config(
        DISPATCH,
        "env = { CANARY = \"${{ secrets.G3_CANARY }}\" }",
        "",
    );
    let message = refusal(&job_env)?;
    assert!(message.contains("env"), "job env: {message}");
    let container = config(
        DISPATCH,
        "[workflow.job.container]\nimage = \"ghcr.io/acme/build@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\nenv = { CANARY = \"${{ secrets.G3_CANARY }}\" }\n",
        "",
    );
    let message = refusal(&container)?;
    assert!(
        message.contains("without expressions"),
        "container env: {message}"
    );
    Ok(())
}

#[test]
fn a_secret_is_refused_in_a_workflow_with_any_other_trigger() -> TestResult {
    let others = [
        (
            "push",
            "\n[workflow.on.push]\nbranches = [\"main\"]\n[workflow.on.workflow_dispatch]\n",
        ),
        (
            "pull_request",
            "\n[workflow.on.pull_request]\ntypes = [\"opened\"]\n[workflow.on.workflow_dispatch]\n",
        ),
        (
            "pull_request",
            "\n[workflow.on]\nschedule = [\"23 4 * * 1\"]\n[workflow.on.pull_request]\ntypes = [\"opened\"]\n",
        ),
        (
            "merge_group",
            "\n[workflow.on]\nmerge_group = true\n[workflow.on.workflow_dispatch]\n",
        ),
        (
            "workflow_run",
            "\n[workflow.on.workflow_run]\nworkflows = [\"Build\"]\n[workflow.on.workflow_dispatch]\n",
        ),
        (
            "workflow_call",
            "\n[workflow.on]\nworkflow_call = true\n[workflow.on.workflow_dispatch]\n",
        ),
        ("workflow_call", ""),
        ("push", "\n[workflow.on.push]\nbranches = [\"main\"]\n"),
    ];
    for (event, on) in others {
        let message = refusal(&config(on, "", &canary()))?;
        assert!(
            message.contains("secrets.G3_CANARY") && message.contains(event),
            "{event}: {message}"
        );
        assert!(message.contains("push to tags"), "{event}: {message}");
    }
    Ok(())
}

#[test]
fn a_secret_name_is_upper_case_letters_digits_and_underscores_and_nothing_else_is_read()
-> TestResult {
    for name in ["G3_CANARY", "A", "A1_B2", "TOKEN"] {
        rendered(&config(
            DISPATCH,
            "",
            &secret_env(&format!("${{{{ secrets.{name} }}}}")),
        ))?;
    }
    for expression in [
        "${{ secrets.g3_canary }}",
        "${{ secrets.1X }}",
        "${{ secrets._X }}",
        "${{ secrets.A-B }}",
        "${{ secrets.A.B }}",
        "${{ secrets.A || secrets.B }}",
        "${{ secrets.A == 'x' }}",
        "${{ secrets['A'] }}",
        "${{ secrets.* }}",
        "${{ toJSON(secrets) }}",
        "${{ secrets }}",
        "${{ Secrets.A }}",
        "x ${{ secrets.A }}",
        "${{ secrets.A }}${{ secrets.B }}",
    ] {
        refusal(&config(DISPATCH, "", &secret_env(expression)))?;
    }
    Ok(())
}

const SCHEDULE: &str = "\n[workflow.on]\nschedule = [\"23 4 * * *\"]\n";
const GATE: &str = "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";

#[test]
fn a_secret_is_allowed_on_trusted_events_only() -> TestResult {
    let trusted = [
        SCHEDULE.to_owned(),
        format!("{SCHEDULE}[workflow.on.workflow_dispatch]\n"),
        "\n[workflow.on.push]\ntags = [\"v*\"]\n".to_owned(),
        "\n[workflow.on.push]\ntags = [\"v*\"]\n[workflow.on.workflow_dispatch]\n".to_owned(),
    ];
    for on in trusted {
        let tree = rendered(&config(&on, "", &canary()))?;
        let hold = steps(job(&tree, "hold")?);
        assert_eq!(
            hold[0].at(&["env", "CANARY"]).and_then(Node::text),
            Some("${{ secrets.G3_CANARY }}"),
            "{on}"
        );
    }
    Ok(())
}

#[test]
fn a_secret_is_refused_when_one_untrusted_event_can_start_the_workflow() -> TestResult {
    let untrusted = [
        (
            "pull_request",
            "[workflow.on.pull_request]\ntypes = [\"opened\"]\n",
        ),
        (
            "merge_group",
            "[workflow.on]\nmerge_group = true\nschedule = [\"23 4 * * *\"]\n",
        ),
        (
            "workflow_run",
            "[workflow.on.workflow_run]\nworkflows = [\"Build\"]\n",
        ),
        ("push", "[workflow.on.push]\nbranches = [\"main\"]\n"),
        (
            "push",
            "[workflow.on.push]\nbranches = [\"main\"]\ntags = [\"v*\"]\n",
        ),
    ];
    for (event, on) in untrusted {
        let on = format!("\n[workflow.on.workflow_dispatch]\n{on}");
        let on = if on.contains("[workflow.on]") {
            on.replace("[workflow.on.workflow_dispatch]\n", "")
        } else {
            on
        };
        let message = refusal(&config(&on, "", &canary()))?;
        assert!(message.contains(event), "{event}: {message}");
    }
    Ok(())
}

#[test]
fn a_push_to_branches_reads_a_secret_only_behind_the_default_branch_gate() -> TestResult {
    let push = "\n[workflow.on.push]\nbranches = [\"main\"]\n";
    let gated = |condition: &str| config(push, &format!("if = \"{condition}\""), &canary());
    for condition in [GATE.to_owned(), format!("{GATE} && github.actor != 'bot'")] {
        rendered(&gated(&condition)).map_err(|error| format!("{condition}: {error}"))?;
    }
    rendered(&workflow(
        &format!("if = \"{GATE}\""),
        push,
        "\n[[workflow.job]]\nid = \"hold\"\n[[workflow.job.step]]\nname = \"Hold\"\nrun = \"sleep 1\"\nenv = { CANARY = \"${{ secrets.G3_CANARY }}\" }\n",
    ))?;
    for condition in [
        format!("{GATE} || true"),
        format!("github.actor == 'x' || {GATE}"),
        format!("github.actor == 'x && {} && y'", GATE.replace('\'', "''")),
        format!("!({GATE})"),
        "github.ref == 'refs/heads/main'".to_owned(),
        GATE.replace("==", "!="),
    ] {
        let message =
            refusal(&gated(&condition)).map_err(|error| format!("{condition}: {error}"))?;
        assert!(message.contains("push"), "{condition}: {message}");
    }
    Ok(())
}

#[test]
fn a_secret_stays_out_of_a_job_env_on_a_scheduled_workflow() -> TestResult {
    let job_env = config(
        SCHEDULE,
        "env = { CANARY = \"${{ secrets.G3_CANARY }}\" }",
        "",
    );
    let message = refusal(&job_env)?;
    assert!(message.contains("env"), "job env: {message}");
    let with = config(SCHEDULE, "", "").replace(
        "run = \"sleep 1\"",
        "uses = \"checkout\"\nwith = { token = \"${{ secrets.G3_CANARY }}\" }",
    );
    let message = refusal(&with)?;
    assert!(message.contains("step env"), "with: {message}");
    Ok(())
}

#[test]
fn a_job_that_reads_a_secret_off_dispatch_holds_no_write_permission() -> TestResult {
    let write = "permissions = { contents = \"write\" }";
    let message = refusal(&config(SCHEDULE, write, &canary()))?;
    assert!(
        message.contains("contents") && message.contains("secret"),
        "{message}"
    );
    rendered(&config(
        SCHEDULE,
        "permissions = { contents = \"read\" }",
        &canary(),
    ))?;
    rendered(&config(SCHEDULE, write, ""))?;
    rendered(&config(DISPATCH, write, &canary()))?;
    Ok(())
}
