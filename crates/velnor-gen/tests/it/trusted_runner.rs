// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::GenerateError;
use velnor_model::RepoView;

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};
use crate::trusted_events::{FORK_RUNNER, NORMAL, TRUSTED, events, runs_on};

const SCALE_SET: &str = "ubuntu-26.04-scale-set";
const POOL: &str = "ubuntu-26.04-scale-set-trusted";
const HELPER: &str = "0.1.109";

fn repo(config: &str, helper: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
    pin_helper(&mut repo, helper)?;
    Ok(repo)
}

fn jobs(
    config: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, Box<dyn std::error::Error>> {
    let yaml: serde_json::Value = crate::yaml_tree::document(&workflow(&repo(config, HELPER)?)?)?;
    Ok(yaml["jobs"].as_object().ok_or("jobs missing")?.clone())
}

fn refused(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(workflow(&repo(config, HELPER)?)
        .err()
        .ok_or_else(|| format!("accepted: {config}"))?
        .to_string())
}

#[test]
fn a_default_branch_event_runs_every_job_on_the_trusted_runner_and_no_other_event_does()
-> TestResult {
    for shape in ["", "shape = 'one-job'\n", "shape = 'groups'\n"] {
        let jobs = jobs(&format!(
            "runner = '{SCALE_SET}'\ntrusted_runner = '{POOL}'\n{shape}"
        ))?;
        assert!(jobs.len() > 1, "{shape}: {jobs:?}");
        for (id, job) in &jobs {
            let expression = job["runs-on"].as_str().ok_or("no runs-on")?;
            for (event, expected) in events() {
                let want = match expected {
                    TRUSTED => POOL,
                    NORMAL => SCALE_SET,
                    FORK_RUNNER => "ubuntu-latest",
                    other => return Err(other.into()),
                };
                assert_eq!(
                    runs_on(expression, &event, &[])?,
                    want,
                    "{shape}: {id}: {} {}",
                    event.name,
                    event.git_ref
                );
            }
        }
    }
    Ok(())
}

#[test]
fn without_the_key_every_job_renders_exactly_as_before() -> TestResult {
    let plain = format!(
        "${{{{ (!startsWith(github.event_name, 'pull_request') || github.event.pull_request.head.repo.full_name == github.repository) && '{SCALE_SET}' || 'ubuntu-latest' }}}}"
    );
    let jobs = jobs(&format!("runner = '{SCALE_SET}'\n"))?;
    assert!(!jobs.is_empty());
    for (id, job) in &jobs {
        assert_eq!(job["runs-on"].as_str(), Some(plain.as_str()), "{id}");
    }
    Ok(())
}

#[test]
fn the_expression_lets_no_pull_request_input_choose_the_trusted_label() -> TestResult {
    let jobs = jobs(&format!(
        "runner = '{SCALE_SET}'\ntrusted_runner = '{POOL}'\n"
    ))?;
    let fence = "(!startsWith(github.event_name, 'pull_request') || github.event.pull_request.head.repo.full_name == github.repository)";
    for (id, job) in &jobs {
        let expression = job["runs-on"].as_str().ok_or("no runs-on")?;
        let (trusted_arm, _) = expression
            .split_once(&format!("'{POOL}'"))
            .ok_or("no trusted label")?;
        for forbidden in [
            "github.head_ref",
            "github.base_ref",
            "github.event.pull_request",
            "github.event.head_commit",
            "github.event.inputs",
            "inputs.",
        ] {
            assert!(!trusted_arm.contains(forbidden), "{id}: {forbidden}");
        }
        let rest = expression.replace(fence, "");
        for forbidden in ["github.head_ref", "github.event.pull_request", "head.ref"] {
            assert!(!rest.contains(forbidden), "{id}: {rest}");
        }
    }
    Ok(())
}

#[test]
fn the_key_needs_a_helper_that_reads_it() -> TestResult {
    let config = format!("runner = '{SCALE_SET}'\ntrusted_runner = '{POOL}'\n");
    for old in ["0.1.106", "0.1.107"] {
        match workflow(&repo(&config, old)?) {
            Err(error) => assert!(
                matches!(
                    error.downcast_ref::<GenerateError>(),
                    Some(GenerateError::TrustedRunnerHelperTooOld(_))
                ),
                "{old}: {error}"
            ),
            Ok(_) => return Err(format!("{old} accepted").into()),
        }
    }
    workflow(&repo(&config, HELPER)?)?;
    workflow(&repo(&format!("runner = '{SCALE_SET}'\n"), "0.1.106")?)?;
    Ok(())
}

#[test]
fn a_trusted_runner_that_could_break_the_fence_or_the_lock_is_refused() -> TestResult {
    for (config, why) in [
        (
            format!("runner = '{SCALE_SET}'\ntrusted_runner = 'ubuntu-latest'\n"),
            "a hosted label",
        ),
        (
            format!("runner = '{SCALE_SET}'\ntrusted_runner = 'ubuntu-26.04'\n"),
            "a hosted label that is not the fork runner",
        ),
        (
            format!("runner = '{SCALE_SET}'\ntrusted_runner = '{SCALE_SET}'\n"),
            "the runner itself",
        ),
        (
            format!("runner = '{SCALE_SET}'\ntrusted_runner = 'Ubuntu-26.04-Scale-Set'\n"),
            "the runner in another case",
        ),
        (
            format!(
                "runner = '{SCALE_SET}'\nfork_runner = 'ubuntu-24.04'\ntrusted_runner = 'ubuntu-24.04'\n"
            ),
            "the fork runner",
        ),
        (
            format!("runner = '{SCALE_SET}'\ntrusted_runner = 'ubuntu-26.04-arm-trusted'\n"),
            "another lock platform",
        ),
        (
            format!("runner = '{SCALE_SET}'\ntrusted_runner = 'macos-trusted'\n"),
            "another lock platform",
        ),
        (
            format!("runner = '{SCALE_SET}'\ntrusted_runner = \"x'y\"\n"),
            "a quote",
        ),
        (
            format!("runner = '{SCALE_SET}'\ntrusted_runner = 'a b'\n"),
            "a space",
        ),
        (
            format!("runner = '{SCALE_SET}'\ntrusted_runner = '${{{{ github.head_ref }}}}'\n"),
            "an expression",
        ),
        (
            format!("runner = '{SCALE_SET}'\ntrusted_runner = ''\n"),
            "empty",
        ),
        (
            "runner = 'ubuntu-24.04'\nhosted = true\ntrusted_runner = 'ubuntu-24.04-trusted'\n"
                .to_owned(),
            "a runner that is hosted",
        ),
        (
            "trusted_runner = 'ubuntu-trusted'\n".to_owned(),
            "no runner, which is hosted",
        ),
    ] {
        let error = refused(&config)?;
        assert!(error.contains("trusted_runner"), "{why}: {error}");
    }
    Ok(())
}

#[test]
fn a_trusted_runner_of_the_same_platform_is_accepted_for_an_unknown_platform_runner() -> TestResult
{
    let jobs = jobs("runner = 'velnor-box'\ntrusted_runner = 'velnor-box-trusted'\n")?;
    let expression = jobs
        .values()
        .next()
        .and_then(|job| job["runs-on"].as_str())
        .ok_or("no runs-on")?;
    assert!(expression.contains("'velnor-box-trusted'"), "{expression}");
    Ok(())
}
