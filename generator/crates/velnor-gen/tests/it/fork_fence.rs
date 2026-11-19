// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const SCALE_SET: &str = "ubuntu-26.04-scale-set";

fn fenced(scale_set: &str, fork: &str) -> String {
    format!(
        "${{{{ (!startsWith(github.event_name, 'pull_request') || github.event.pull_request.head.repo.full_name == github.repository) && '{scale_set}' || '{fork}' }}}}"
    )
}

fn jobs(
    config: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, Box<dyn std::error::Error>> {
    let repo = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
    let yaml: serde_json::Value = crate::yaml_tree::document(&workflow(&repo)?)?;
    Ok(yaml["jobs"].as_object().ok_or("jobs missing")?.clone())
}

#[test]
fn every_self_hosted_job_runs_on_a_hosted_runner_for_fork_pull_requests() -> TestResult {
    for shape in [
        "",
        "shape = 'one-job'\n",
        "shape = 'lint-test'\n",
        "shape = 'groups'\n",
    ] {
        let jobs = jobs(&format!("runner = '{SCALE_SET}'\n{shape}"))?;
        assert!(jobs.len() > 1, "{shape}: {jobs:?}");
        for (id, job) in &jobs {
            assert_eq!(
                job["runs-on"].as_str(),
                Some(fenced(SCALE_SET, "ubuntu-latest").as_str()),
                "{shape}: {id}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_hosted_runner_needs_no_fence() -> TestResult {
    for runner in ["ubuntu-latest", "ubuntu-24.04"] {
        let jobs = jobs(&format!("runner = '{runner}'\n"))?;
        for (id, job) in &jobs {
            assert_eq!(job["runs-on"].as_str(), Some(runner), "{id}");
        }
    }
    Ok(())
}

#[test]
fn the_fork_runner_is_configurable_and_must_be_hosted_and_match_the_lock_platform() -> TestResult {
    let jobs = jobs(&format!(
        "runner = '{SCALE_SET}'\nfork_runner = 'ubuntu-24.04'\n"
    ))?;
    for (id, job) in &jobs {
        assert_eq!(
            job["runs-on"].as_str(),
            Some(fenced(SCALE_SET, "ubuntu-24.04").as_str()),
            "{id}"
        );
    }
    for fork_runner in [SCALE_SET, "macos-15", "ubuntu-24.04-arm", "x'y"] {
        let repo = view(
            &["Cargo.toml"],
            Some(format!("runner = '{SCALE_SET}'\nfork_runner = \"{fork_runner}\"\n").as_bytes()),
        )?;
        let error = workflow(&repo)
            .err()
            .ok_or_else(|| format!("fork_runner {fork_runner} accepted"))?
            .to_string();
        assert!(error.contains("fork_runner"), "{error}");
    }
    Ok(())
}

#[test]
fn the_gate_fails_closed_for_forks() -> TestResult {
    for shape in ["", "shape = 'groups'\n"] {
        let jobs = jobs(&format!("runner = '{SCALE_SET}'\n{shape}"))?;
        let gate = jobs.get("required").ok_or("gate missing")?;
        assert_eq!(
            gate["runs-on"].as_str(),
            Some(fenced(SCALE_SET, "ubuntu-latest").as_str()),
            "{shape}"
        );
        assert!(
            gate["if"]
                .as_str()
                .is_some_and(|condition| condition.ends_with("&& (always()) }}")),
            "{gate}"
        );
        assert!(
            gate["needs"]
                .as_array()
                .is_some_and(|needs| !needs.is_empty())
        );
        for (id, job) in &jobs {
            let condition = job["if"].as_str().unwrap_or_default();
            assert!(
                !condition.contains("head.repo") && !condition.contains("fork"),
                "{shape}: {id} skips fork pull requests instead of running them on a hosted runner: {condition}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_self_hosted_label_that_could_break_out_of_the_runs_on_expression_is_refused() -> TestResult {
    for runner in [
        "${{ secrets.TOKEN }}",
        "x' || secrets.TOKEN || 'y",
        "a b",
        "a}}",
    ] {
        let repo = view(
            &["Cargo.toml"],
            Some(format!("runner = \"{runner}\"\n").as_bytes()),
        )?;
        let error = workflow(&repo)
            .err()
            .ok_or_else(|| format!("runner {runner} accepted"))?
            .to_string();
        assert!(error.contains("runner"), "{error}");
    }
    Ok(())
}

#[test]
fn only_exact_github_hosted_labels_skip_the_fork_fence() {
    for runner in [
        "ubuntu-24.04-arm",
        "windows-11-arm",
        "ubuntu-latest",
        "macos-15",
    ] {
        assert!(velnor_model::github_hosted(runner), "{runner}");
    }
    for runner in [
        "ubuntu-26.04-arm-scale-set",
        "ubuntu-24.04-armada",
        "ubuntu-26.04-scale-set",
        "ubuntu-24.04-arm-",
    ] {
        assert!(!velnor_model::github_hosted(runner), "{runner}");
    }
}
