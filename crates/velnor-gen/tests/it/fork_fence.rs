// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::github_hosted;

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
fn unknown_primary_platform_accepts_only_a_linux_x64_fork_runner() -> TestResult {
    let default = jobs("runner = 'velnor-box'\n")?;
    for (id, job) in &default {
        assert_eq!(
            job["runs-on"].as_str(),
            Some(fenced("velnor-box", "ubuntu-latest").as_str()),
            "{id}"
        );
    }
    let explicit = jobs("runner = 'velnor-box'\nfork_runner = 'ubuntu-24.04'\n")?;
    assert!(explicit.values().all(|job| {
        job["runs-on"].as_str() == Some(fenced("velnor-box", "ubuntu-24.04").as_str())
    }));
    for fork_runner in ["windows-latest", "macos-15", "ubuntu-24.04-arm"] {
        let repo = view(
            &["Cargo.toml"],
            Some(format!("runner = 'velnor-box'\nfork_runner = \"{fork_runner}\"\n").as_bytes()),
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
fn github_hosted_accepts_every_listed_label() {
    for label in [
        "ubuntu-latest",
        "ubuntu-24.04",
        "ubuntu-22.04",
        "ubuntu-24.04-arm",
        "ubuntu-22.04-arm",
        "ubuntu-26.04",
        "ubuntu-26.04-arm",
        "ubuntu-slim",
        "macos-latest",
        "macos-latest-large",
        "macos-26",
        "macos-26-xlarge",
        "macos-26-intel",
        "macos-26-large",
        "macos-15",
        "macos-15-large",
        "macos-15-xlarge",
        "macos-15-intel",
        "macos-14",
        "windows-latest",
        "windows-2025",
        "windows-2025-vs2026",
        "windows-2022",
        "windows-11-arm",
        "windows-11-vs2026-arm",
        "Ubuntu-Latest",
    ] {
        assert!(github_hosted(label), "{label}");
    }
}

#[test]
fn github_hosted_rejects_every_look_alike_label() {
    for label in [
        "ubuntu-",
        "ubuntu-999.99",
        "ubuntu-24.04-armx",
        "ubuntu-26.04-scale-set",
        "windows-2019",
        "macos-13",
        "velnor-box",
        "",
    ] {
        assert!(!github_hosted(label), "{label}");
    }
}

#[test]
fn the_docs_do_not_claim_the_fence_is_a_security_boundary() {
    let docs = include_str!("../../../../generator/CONFIG.md");
    assert!(!docs.contains("must never run on a self-hosted runner"));
    assert!(docs.contains("Require approval for all outside collaborators"));
    assert!(docs.contains("runner group"));
}

#[test]
fn a_fork_fenced_workflow_never_listens_for_privileged_triggers() -> TestResult {
    for shape in [
        "",
        "shape = 'one-job'\n",
        "shape = 'lint-test'\n",
        "shape = 'groups'\n",
    ] {
        for runner in [SCALE_SET, "ubuntu-latest"] {
            let repo = view(
                &["Cargo.toml", "tools/Cargo.toml"],
                Some(format!("runner = '{runner}'\n{shape}").as_bytes()),
            )?;
            let yaml: serde_json::Value = crate::yaml_tree::document(&workflow(&repo)?)?;
            let triggers = yaml["on"].as_object().ok_or("on missing")?;
            assert!(
                triggers.contains_key("pull_request"),
                "{shape}: {triggers:?}"
            );
            for trigger in ["pull_request_target", "workflow_run"] {
                assert!(
                    !triggers.contains_key(trigger),
                    "{shape}: {runner}: {trigger}"
                );
            }
        }
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
