// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::{RepoPath, RepoView};

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const SCALE_SET: &str = "ubuntu-26.04-scale-set";
const MISE: &str = "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\njactionlint = \"2.4.0\"\nshellcheck = \"0.11.0\"\n";
pub(crate) const LINT: &str = "[[lint]]\nkind = 'jactionlint'\nconfig = '.github/jactionlint.yaml'\n\n[[lint]]\nkind = 'alint'\nconfig = '.alint.yml'\nprobe = 'scripts/alint-line-cap-probe.sh'\n\n[[lint]]\nkind = 'mise-task'\ntask = 'lint-generator-samples'\ntools = ['jactionlint']\ntimeout_minutes = 5\n";
pub(crate) const IDS: [&str; 3] = [
    "lint-jactionlint",
    "lint-alint",
    "lint-lint-generator-samples",
];

pub(crate) fn repo(config: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    repo.files
        .insert(RepoPath::new("mise.toml")?, MISE.as_bytes().to_vec());
    Ok(repo)
}

pub(crate) fn rendered(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    workflow(&repo(config)?)
}

fn jobs(config: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(crate::yaml_tree::document(&rendered(config)?)?["jobs"].clone())
}

pub(crate) fn runs(job: &Value) -> Vec<String> {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|step| step["run"].as_str().map(str::to_owned))
        .collect()
}

fn uses(job: &Value) -> Vec<String> {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|step| step["uses"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn every_lint_entry_renders_one_fenced_least_privilege_job_that_waits_for_nothing() -> TestResult {
    let jobs = jobs(&format!("runner = '{SCALE_SET}'\n{LINT}"))?;
    let fence = "${{ (!startsWith(github.event_name, 'pull_request') || github.event.pull_request.head.repo.full_name == github.repository) && 'ubuntu-26.04-scale-set' || 'ubuntu-latest' }}";
    for id in IDS {
        let job = &jobs[id];
        assert!(job.is_object(), "{id} missing");
        assert_eq!(job["runs-on"].as_str(), Some(fence), "{id}");
        assert_eq!(
            job["permissions"],
            serde_json::json!({"contents": "read"}),
            "{id}"
        );
        assert!(job["needs"].is_null(), "{id} must not wait for the plan");
        assert!(job["environment"].is_null(), "{id}");
        assert!(job["if"].as_str().is_some(), "{id}");
        assert_eq!(
            job["steps"][0]["with"]["persist-credentials"], false,
            "{id}"
        );
        assert!(
            uses(job).iter().all(
                |action| action.split_once('@').is_some_and(|(_, reference)| {
                    reference.len() == 40 && reference.bytes().all(|b| b.is_ascii_hexdigit())
                })
            ),
            "{id}: {:?}",
            uses(job)
        );
    }
    assert_eq!(jobs["lint-lint-generator-samples"]["timeout-minutes"], 5);
    assert_eq!(jobs["lint-alint"]["timeout-minutes"], 10);
    Ok(())
}

#[test]
fn a_hosted_runner_needs_no_fence_for_lint_jobs() -> TestResult {
    let jobs = jobs(&format!("runner = 'ubuntu-24.04'\n{LINT}"))?;
    for id in IDS {
        assert_eq!(jobs[id]["runs-on"].as_str(), Some("ubuntu-24.04"), "{id}");
    }
    Ok(())
}

#[test]
fn each_lint_job_runs_its_tool_as_one_command_or_one_pinned_action() -> TestResult {
    let jobs = jobs(LINT)?;
    assert_eq!(
        runs(&jobs["lint-jactionlint"]),
        [
            "jactionlint --config-file .github/jactionlint.yaml --format github --strict-exit --no-online --no-color --pyflakes= --shellcheck=shellcheck"
        ]
    );
    let install = &jobs["lint-jactionlint"]["steps"][1];
    assert_eq!(install["with"]["install_args"], "jactionlint shellcheck");
    assert_eq!(
        runs(&jobs["lint-alint"]),
        ["bash scripts/alint-line-cap-probe.sh"]
    );
    let alint = &jobs["lint-alint"]["steps"][1];
    assert_eq!(alint["with"]["config"], ".alint.yml");
    assert_eq!(alint["with"]["fail-on-warning"], true);
    assert!(
        alint["uses"]
            .as_str()
            .is_some_and(|uses| uses.starts_with("asamarts/alint@"))
    );
    assert_eq!(
        runs(&jobs["lint-lint-generator-samples"]),
        ["mise run --skip-tools lint-generator-samples"]
    );
    let install = &jobs["lint-lint-generator-samples"]["steps"][1];
    assert_eq!(install["with"]["install_args"], "jactionlint");
    Ok(())
}

#[test]
fn a_mise_task_without_tools_installs_mise_but_no_tools() -> TestResult {
    let config = "[[lint]]\nkind = 'mise-task'\ntask = 'check'\n";
    let jobs = jobs(config)?;
    let install = &jobs["lint-check"]["steps"][1];
    assert_eq!(install["with"]["install"], false);
    assert!(install["with"]["install_args"].is_null());
    Ok(())
}

#[test]
fn a_repository_without_lint_entries_renders_no_lint_job() -> TestResult {
    let jobs = jobs("")?;
    assert!(
        jobs.as_object()
            .is_some_and(|jobs| jobs.keys().all(|id| !id.starts_with("lint-")))
    );
    Ok(())
}
