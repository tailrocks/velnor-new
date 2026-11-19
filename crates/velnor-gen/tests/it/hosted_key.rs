// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A runner is open to fork pull requests only when `hosted = true` is declared for it.
//! The spelling of a label decides nothing, so a self-hosted runner registered under a
//! GitHub-hosted name is still fenced.

use crate::declared_fixture::{JOB, job, refusal, rendered, workflow as declared};
use crate::facts::TestResult;
use crate::pipeline::{view, workflow};
use crate::yaml_tree::Node;

const PUSH: &str = "[workflow.on.push]\nbranches = [\"main\"]\n";

fn runs_on(config: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let repo = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
    let yaml: serde_json::Value = crate::yaml_tree::document(&workflow(&repo)?)?;
    let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
    assert!(!jobs.is_empty());
    Ok(jobs
        .values()
        .map(|job| job["runs-on"].as_str().unwrap_or_default().to_owned())
        .collect())
}

fn refused(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    Ok(workflow(&repo)
        .err()
        .ok_or_else(|| format!("accepted: {config}"))?
        .to_string())
}

fn is_fenced(runs_on: &str) -> bool {
    runs_on.contains("github.event.pull_request.head.repo.full_name == github.repository")
}

#[test]
fn hosted_false_on_a_github_label_is_fenced() -> TestResult {
    let jobs = runs_on("runner = 'ubuntu-26.04'\nhosted = false\n")?;
    assert!(jobs.iter().all(|label| is_fenced(label)), "{jobs:?}");
    assert!(jobs.iter().all(|label| label.contains("'ubuntu-26.04'")));
    Ok(())
}

#[test]
fn hosted_true_on_a_custom_label_is_open() -> TestResult {
    let jobs = runs_on("runner = 'my-box'\nhosted = true\n")?;
    assert!(jobs.iter().all(|label| label == "my-box"), "{jobs:?}");
    Ok(())
}

#[test]
fn hosted_true_on_a_github_label_is_open() -> TestResult {
    let jobs = runs_on("runner = 'ubuntu-26.04'\nhosted = true\n")?;
    assert!(jobs.iter().all(|label| label == "ubuntu-26.04"), "{jobs:?}");
    Ok(())
}

#[test]
fn an_undeclared_custom_label_is_fenced_without_an_error() -> TestResult {
    let jobs = runs_on("runner = 'my-box'\n")?;
    assert!(jobs.iter().all(|label| is_fenced(label)), "{jobs:?}");
    Ok(())
}

#[test]
fn an_undeclared_scale_set_label_is_fenced_and_never_matches_by_prefix() -> TestResult {
    let jobs = runs_on("runner = 'ubuntu-26.04-scale-set'\n")?;
    assert!(jobs.iter().all(|label| is_fenced(label)), "{jobs:?}");
    Ok(())
}

#[test]
fn an_undeclared_github_label_is_a_generate_error() -> TestResult {
    for label in ["ubuntu-26.04", "ubuntu-latest", "macos-15", "Ubuntu-24.04"] {
        let message = refused(&format!("runner = '{label}'\n"))?;
        assert!(
            message.contains(label)
                && message.contains("looks GitHub-hosted")
                && message.contains("`hosted = true`")
                && message.contains("`hosted = false`"),
            "{message}"
        );
    }
    Ok(())
}

#[test]
fn the_default_runner_is_hosted_unless_declared_otherwise() -> TestResult {
    assert!(runs_on("")?.iter().all(|label| label == "ubuntu-latest"));
    let jobs = runs_on("hosted = false\n")?;
    assert!(jobs.iter().all(|label| is_fenced(label)), "{jobs:?}");
    Ok(())
}

#[test]
fn one_label_cannot_be_both_hosted_and_self_hosted() -> TestResult {
    let mirror = "[crates_mirror]\nrunner = 'my-box'\nregistry = 'sparse+https://m.example/index/'\ncainfo = '/ca.pem'\n";
    let message = refused(&format!("runner = 'my-box'\nhosted = true\n{mirror}"))?;
    assert!(
        message.contains("my-box") && message.contains("both hosted"),
        "{message}"
    );
    let group = "[[test_groups]]\nname = 'slow'\nfilter = 'test(~slow)'\nrunner = 'ubuntu-24.04'\nhosted = true\n";
    let message = refused(&format!("runner = 'ubuntu-24.04'\nhosted = false\n{group}"))?;
    assert!(message.contains("both hosted"), "{message}");
    let alone = "[[test_groups]]\nname = 'slow'\nfilter = 'test(~slow)'\nhosted = true\n";
    assert!(refused(alone)?.contains("without `runner`"));
    Ok(())
}

#[test]
fn a_declared_workflow_follows_the_same_rule() -> TestResult {
    let fork = "[workflow.on.pull_request]\ntypes = [\"opened\"]\n";
    let tree = |header: &str| rendered(&declared(header, fork, JOB));
    let runs = |header: &str| -> Result<String, Box<dyn std::error::Error>> {
        Ok(job(&tree(header)?, "check")?
            .get("runs-on")
            .and_then(Node::text)
            .ok_or("no runs-on")?
            .to_owned())
    };
    assert_eq!(
        runs("runner = \"ubuntu-26.04\"\nhosted = true")?,
        "ubuntu-26.04"
    );
    assert_eq!(runs("runner = \"my-box\"\nhosted = true")?, "my-box");
    assert!(is_fenced(&runs(
        "runner = \"ubuntu-26.04\"\nhosted = false"
    )?));
    assert!(is_fenced(&runs("runner = \"my-box\"")?));
    assert!(is_fenced(&runs("runner = \"ubuntu-26.04-scale-set\"")?));
    let message = refusal(&declared("runner = \"ubuntu-26.04\"", PUSH, JOB))?;
    assert!(
        message.contains("looks GitHub-hosted") && message.contains("`hosted = true`"),
        "{message}"
    );
    Ok(())
}

#[test]
fn the_hosted_key_needs_the_helper_that_reads_it_at_run_time() -> TestResult {
    let config = b"runner = 'my-box'\nhosted = false\n";
    let mut repo = view(&["Cargo.toml"], Some(config))?;
    assert!(workflow(&repo).is_ok());
    crate::pipeline::pin_helper(&mut repo, "0.1.97")?;
    let message = workflow(&repo).err().ok_or("0.1.97 accepted")?.to_string();
    assert!(
        message.contains("pin velnor-ci 0.1.98 or later"),
        "{message}"
    );
    let plain = view(&["Cargo.toml"], Some(b"runner = 'my-box'\n"))?;
    assert!(
        workflow(&plain).is_ok(),
        "an undeclared label needs no new helper"
    );
    Ok(())
}
