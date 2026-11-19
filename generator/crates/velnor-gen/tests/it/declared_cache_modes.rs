// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, job, refusal, rendered, workflow};
use crate::yaml_tree::Node;

const GATE: &str = "github.ref != format('refs/heads/{0}', github.event.repository.default_branch)";

fn one_job(header: &str, steps: &str) -> String {
    workflow(
        "",
        "",
        &format!("\n[[workflow.job]]\nid = \"check\"\n{header}\n{steps}"),
    )
}

fn run_step() -> &'static str {
    "[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\n"
}

fn with_step(uses: &str, with: &str) -> String {
    format!("[[workflow.job.step]]\nname = \"Step\"\nuses = \"{uses}\"\nwith = {with}\n")
}

#[test]
fn a_job_env_is_written_before_the_steps() -> TestResult {
    let config = one_job(
        "env = { MISE_NO_CONFIG = \"1\", WORK = \"${{ github.workspace }}/probe\" }\n",
        run_step(),
    );
    let tree = rendered(&config)?;
    let env = job(&tree, "check")?.get("env").ok_or("no env")?;
    assert_eq!(env.get("MISE_NO_CONFIG").and_then(Node::text), Some("1"));
    assert_eq!(
        env.get("WORK").and_then(Node::text),
        Some("${{ github.workspace }}/probe")
    );
    let keys = job(&tree, "check")?.keys();
    let at = |key: &str| keys.iter().position(|seen| *seen == key);
    assert!(at("env") < at("steps"), "{keys:?}");
    Ok(())
}

#[test]
fn a_job_env_refuses_expressions_other_than_the_workspace() -> TestResult {
    for value in [
        "${{ secrets.TOKEN }}",
        "${{ github.event.pull_request.title }}",
        "${{ inputs.mode }}",
        "x-${{ github.ref_name }}",
        "x-${{ github.workspace",
    ] {
        let config = one_job(&format!("env = {{ NAME = \"{value}\" }}\n"), run_step());
        let message = refusal(&config)?;
        assert!(message.contains("env"), "{value}: {message}");
        assert!(
            message.contains("github.workspace"),
            "{value}: the message names the allowed expression: {message}"
        );
    }
    let message = refusal(&one_job("env = { \"BAD-NAME\" = \"1\" }\n", run_step()))?;
    assert!(message.contains("BAD-NAME"), "{message}");
    Ok(())
}

#[test]
fn a_job_cache_mode_is_written_as_the_github_key() -> TestResult {
    for mode in ["read", "none"] {
        let config = one_job(&format!("cache_mode = \"{mode}\"\n"), run_step());
        let tree = rendered(&config)?;
        let body = job(&tree, "check")?;
        assert_eq!(body.get("cache-mode").and_then(Node::text), Some(mode));
        let keys = body.keys();
        let at = |key: &str| keys.iter().position(|seen| *seen == key);
        assert!(at("cache-mode") < at("steps"), "{keys:?}");
    }
    let plain = rendered(&one_job("", run_step()))?;
    assert!(job(&plain, "check")?.get("cache-mode").is_none());
    Ok(())
}

#[test]
fn a_job_cache_mode_is_a_literal_github_accepts() -> TestResult {
    for value in ["bogus", "${{ inputs.mode }}", "Write", ""] {
        let config = one_job(&format!("cache_mode = \"{value}\"\n"), run_step());
        let message = refusal(&config)?;
        assert!(
            message.contains("cache_mode") && message.contains("write-only"),
            "{value}: {message}"
        );
    }
    Ok(())
}

#[test]
fn a_cache_writer_must_be_gated_off_the_default_branch() -> TestResult {
    let ungated = one_job("cache_mode = \"write\"\n", run_step());
    let message = refusal(&ungated)?;
    assert!(message.contains("default_branch"), "{message}");
    let wrong = one_job(
        "cache_mode = \"write\"\nif = \"github.ref == 'refs/heads/x'\"\n",
        run_step(),
    );
    assert!(refusal(&wrong)?.contains("default_branch"));
    let gated = one_job(
        &format!("cache_mode = \"write\"\nif = \"{GATE}\"\n"),
        run_step(),
    );
    let tree = rendered(&gated)?;
    assert_eq!(
        job(&tree, "check")?.get("cache-mode").and_then(Node::text),
        Some("write")
    );
    let write_only = one_job("cache_mode = \"write-only\"\n", run_step());
    assert!(refusal(&write_only)?.contains("default_branch"));
    Ok(())
}

#[test]
fn the_cache_actions_render_at_their_pins() -> TestResult {
    let steps = format!(
        "{}{}{}",
        with_step("cache", "{ path = \"a\", key = \"k\" }"),
        with_step("cache-save", "{ path = \"a\", key = \"k\" }"),
        with_step("cache-restore", "{ path = \"a\", key = \"k\" }"),
    );
    let tree = rendered(&one_job("", &steps))?;
    let uses: Vec<&str> = job(&tree, "check")?
        .get("steps")
        .ok_or("no steps")?
        .items()
        .iter()
        .filter_map(|step| step.get("uses").and_then(Node::text))
        .collect();
    let pin = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
    assert_eq!(
        uses,
        [
            format!("actions/cache@{pin}"),
            format!("actions/cache/save@{pin}"),
            format!("actions/cache/restore@{pin}")
        ]
    );
    Ok(())
}

#[test]
fn a_with_value_may_interpolate_the_run_and_job_identity() -> TestResult {
    let key = "g4-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.job }}";
    let config = one_job(
        "",
        &with_step(
            "cache-save",
            &format!("{{ path = \"a b/c\", key = \"{key}\" }}"),
        ),
    );
    let tree = rendered(&config)?;
    let step = &job(&tree, "check")?.get("steps").ok_or("no steps")?.items()[0];
    assert_eq!(step.at(&["with", "key"]).and_then(Node::text), Some(key));
    assert_eq!(
        step.at(&["with", "path"]).and_then(Node::text),
        Some("a b/c")
    );
    Ok(())
}

#[test]
fn a_with_value_refuses_every_other_interpolated_expression() -> TestResult {
    for expression in [
        "secrets.TOKEN",
        "github.event.pull_request.head.ref",
        "inputs.mode",
        "github.ref_name",
        "github.sha",
        "env.NAME",
        "format('{0}', github.run_id)",
        "github.run_id || github.job",
    ] {
        let config = one_job(
            "",
            &with_step(
                "cache-save",
                &format!("{{ path = \"a\", key = \"k-${{{{ {expression} }}}}\" }}"),
            ),
        );
        let message = refusal(&config)?;
        assert!(message.contains(expression), "{expression}: {message}");
        assert!(
            message.contains("github.run_id")
                && message.contains("github.run_attempt")
                && message.contains("github.job"),
            "{expression}: the message names the allowed expressions: {message}"
        );
    }
    let open = one_job(
        "",
        &with_step(
            "cache-save",
            "{ path = \"a\", key = \"k-${{ github.run_id\" }",
        ),
    );
    assert!(refusal(&open)?.contains("${{"));
    Ok(())
}

#[test]
fn an_env_value_of_a_step_still_cannot_interpolate() -> TestResult {
    let config = one_job(
        "",
        "[[workflow.job.step]]\nname = \"Check\"\nrun = \"echo ok\"\nenv = { KEY = \"k-${{ github.run_id }}\" }\n",
    );
    let message = refusal(&config)?;
    assert!(message.contains("embeds an expression"), "{message}");
    Ok(())
}

#[test]
fn a_cache_key_is_measured_with_the_longest_job_id_and_attempt_it_can_hold() -> TestResult {
    let padding = "k".repeat(420);
    for uses in ["cache", "cache-save", "cache-restore"] {
        let key = format!("{padding}-${{{{ github.job }}}}-${{{{ github.run_attempt }}}}");
        let config = one_job(
            "",
            &with_step(uses, &format!("{{ path = \"a\", key = \"{key}\" }}")),
        );
        let message = refusal(&config)?;
        assert!(message.contains("cache key"), "{uses}: {message}");
        let short = format!("{}-${{{{ github.job }}}}", "k".repeat(300));
        let config = one_job(
            "",
            &with_step(uses, &format!("{{ path = \"a\", key = \"{short}\" }}")),
        );
        rendered(&config)?;
    }
    Ok(())
}
