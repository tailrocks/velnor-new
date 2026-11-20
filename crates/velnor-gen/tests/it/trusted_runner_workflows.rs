// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{JOB, TestResult, job, refusal, rendered_file, workflow};
use crate::trusted_events::{Event, FORK_RUNNER, NORMAL, TRUSTED, events, runs_on};
use crate::yaml_tree::Node;

const POOL: &str = "velnor-box-trusted";
const FORK: &str = "ubuntu-24.04";
const ON: &str = "[workflow.on.workflow_dispatch]\n";
const FENCE: &str = "(!startsWith(github.event_name, 'pull_request') || github.event.pull_request.head.repo.full_name == github.repository)";

const CASES: &str = r#"
[[workflow]]
file = "qualification"
name = "Qualification"
hosted_runner = "ubuntu-24.04"
scale_set_runner = "velnor-box"
fork_runner = "ubuntu-24.04"
TRUSTED
[workflow.on.workflow_dispatch]

[[workflow.on.workflow_dispatch.input]]
name = "mode"
type = "choice"
options = ["both", "js"]
default = "both"

[[workflow.case]]
id = "js"
timeout_minutes = 5

[[workflow.case.step]]
name = "Run"
run = "echo js"
"#;

fn expression(config: &str, file: &str, id: &str) -> Result<String, Box<dyn std::error::Error>> {
    let tree = rendered_file(config, file)?;
    Ok(job(&tree, id)?
        .get("runs-on")
        .and_then(Node::text)
        .ok_or("no runs-on")?
        .to_owned())
}

fn expected(outcome: &str, normal: &'static str) -> &'static str {
    match outcome {
        TRUSTED => POOL,
        NORMAL => normal,
        FORK_RUNNER => FORK,
        _ => "unreachable",
    }
}

fn check(
    expression: &str,
    normal: &'static str,
    matrix: &[(&str, &str)],
    skip: impl Fn(&Event) -> bool,
) -> TestResult {
    for (event, outcome) in events() {
        if skip(&event) {
            continue;
        }
        assert_eq!(
            runs_on(expression, &event, matrix)?,
            expected(outcome, normal),
            "{} {}: {expression}",
            event.name,
            event.git_ref
        );
    }
    Ok(())
}

#[test]
fn the_scale_set_job_of_a_case_runs_on_the_trusted_runner_for_default_branch_events_only()
-> TestResult {
    let config = CASES.replace("TRUSTED", &format!("trusted_runner = \"{POOL}\""));
    check(
        &expression(&config, "qualification", "js-scale-set")?,
        "velnor-box",
        &[],
        |_| false,
    )?;
    assert_eq!(expression(&config, "qualification", "js-hosted")?, FORK);
    Ok(())
}

#[test]
fn without_the_key_the_case_jobs_render_as_before() -> TestResult {
    let config = CASES.replace("TRUSTED", "");
    assert_eq!(
        expression(&config, "qualification", "js-scale-set")?,
        format!("${{{{ {FENCE} && 'velnor-box' || 'ubuntu-24.04' }}}}")
    );
    assert_eq!(expression(&config, "qualification", "js-hosted")?, FORK);
    Ok(())
}

#[test]
fn a_plain_self_hosted_workflow_routes_each_job_by_event() -> TestResult {
    let config = workflow(
        &format!("runner = \"velnor-box\"\ntrusted_runner = \"{POOL}\"\nfork_runner = \"{FORK}\""),
        ON,
        JOB,
    );
    check(
        &expression(&config, "ci", "check")?,
        "velnor-box",
        &[],
        |_| false,
    )
}

#[test]
fn a_job_on_another_self_hosted_runner_or_a_hosted_one_keeps_its_label() -> TestResult {
    let jobs = format!(
        "{JOB}\n[[workflow.job]]\nid = \"other\"\nrunner = \"other-box\"\n\n[[workflow.job.step]]\nname = \"Other\"\nrun = \"echo ok\"\n\n[[workflow.job]]\nid = \"hosted\"\nrunner = \"ubuntu-24.04\"\nhosted = true\n\n[[workflow.job.step]]\nname = \"Hosted\"\nrun = \"echo ok\"\n"
    );
    let config = workflow(
        &format!("runner = \"velnor-box\"\ntrusted_runner = \"{POOL}\"\nfork_runner = \"{FORK}\""),
        ON,
        &jobs,
    );
    let other = expression(&config, "ci", "other")?;
    assert!(!other.contains(POOL), "{other}");
    assert_eq!(expression(&config, "ci", "hosted")?, "ubuntu-24.04");
    assert!(expression(&config, "ci", "check")?.contains(POOL));
    Ok(())
}

#[test]
fn a_matrix_axis_swaps_only_the_scale_set_value() -> TestResult {
    let jobs = "\n[[workflow.job]]\nid = \"build\"\nrunner = \"${{ matrix.runner }}\"\nrunner_labels = { \"macos-15\" = true, \"velnor-box\" = false }\n\n[workflow.job.matrix]\naxes = { runner = [\"macos-15\", \"velnor-box\"] }\n\n[[workflow.job.step]]\nname = \"Build\"\nrun = \"echo ok\"\n";
    let config = workflow(
        &format!("runner = \"velnor-box\"\ntrusted_runner = \"{POOL}\"\nfork_runner = \"{FORK}\""),
        ON,
        jobs,
    );
    let text = expression(&config, "ci", "build")?;
    check(&text, "velnor-box", &[("runner", "velnor-box")], |_| false)?;
    for (event, _) in events() {
        assert_eq!(
            runs_on(&text, &event, &[("runner", "macos-15")])?.eq_ignore_ascii_case("macos-15"),
            event.head_repository != Some(crate::trusted_events::FORK),
            "{} {}: {text}",
            event.name,
            event.git_ref
        );
    }
    Ok(())
}

#[test]
fn a_trusted_runner_that_cannot_be_a_pool_of_the_workflow_is_refused() -> TestResult {
    for (header, why) in [
        (
            "runner = \"velnor-box\"\ntrusted_runner = \"ubuntu-latest\"",
            "a hosted label",
        ),
        (
            "runner = \"velnor-linux-x64\"\ntrusted_runner = \"ubuntu-26.04\"",
            "a hosted label of the same platform that is not the fork runner",
        ),
        (
            "runner = \"velnor-box\"\ntrusted_runner = \"velnor-box\"",
            "the runner",
        ),
        (
            "runner = \"velnor-box\"\ntrusted_runner = \"Velnor-Box\"",
            "the runner in another case",
        ),
        (
            "runner = \"velnor-box\"\nfork_runner = \"ubuntu-24.04\"\ntrusted_runner = \"ubuntu-24.04\"",
            "the fork runner",
        ),
        (
            "runner = \"velnor-box\"\ntrusted_runner = \"ubuntu-24.04-arm-trusted\"",
            "another platform",
        ),
        (
            "runner = \"velnor-box\"\ntrusted_runner = \"a'b\"",
            "a quote",
        ),
        (
            "runner = \"velnor-box\"\ntrusted_runner = \"${{ github.head_ref }}\"",
            "an expression",
        ),
        ("runner = \"velnor-box\"\ntrusted_runner = \"\"", "empty"),
        (
            "trusted_runner = \"velnor-box-trusted\"",
            "a workflow on the default hosted runner",
        ),
        (
            "runner = \"ubuntu-24.04\"\nhosted = true\ntrusted_runner = \"velnor-box-trusted\"",
            "a hosted runner",
        ),
    ] {
        let message = refusal(&workflow(header, ON, JOB))?;
        assert!(message.contains("trusted_runner"), "{why}: {message}");
    }
    let cases = CASES.replace("TRUSTED", "trusted_runner = \"ubuntu-24.04\"");
    assert!(refusal(&cases)?.contains("trusted_runner"));
    Ok(())
}

#[test]
fn a_workflow_whose_dispatch_can_choose_what_to_build_cannot_use_the_pool() -> TestResult {
    let header = format!("runner = \"velnor-box\"\ntrusted_runner = \"{POOL}\"");
    for name in ["ref", "sha", "repository", "Ref"] {
        let on = format!(
            "[workflow.on.workflow_dispatch]\n\n[[workflow.on.workflow_dispatch.input]]\nname = \"{name}\"\ntype = \"string\"\n"
        );
        let message = refusal(&workflow(&header, &on, JOB))?;
        assert!(message.contains("trusted_runner"), "{name}: {message}");
    }
    let checkout = "\n[[workflow.job]]\nid = \"build\"\n\n[[workflow.job.step]]\nname = \"Checkout\"\nuses = \"actions/checkout\"\n\n[workflow.job.step.with]\nref = \"${{ inputs.target }}\"\n";
    let on = "[workflow.on.workflow_dispatch]\n\n[[workflow.on.workflow_dispatch.input]]\nname = \"target\"\ntype = \"string\"\n";
    let message = refusal(&workflow(&header, on, checkout))?;
    assert!(message.contains("trusted_runner"), "{message}");
    Ok(())
}
