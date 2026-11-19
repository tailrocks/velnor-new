// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::TestResult;
use super::fixture::{ApiPlan, GitFixture};
use super::support::qualify;

#[test]
fn public_qualifier_rejects_missing_and_invalid_tool_versions_before_docker() {
    let cases = [
        ("buildx_version", None),
        ("buildx_version", Some("not-a-version")),
        ("compose_version", None),
        ("compose_version", Some("not-a-version")),
        ("node_version", None),
        ("node_version", Some("not-a-version")),
    ];
    let mut failures = Vec::new();
    let mut outcomes = Vec::new();
    for (key, value) in cases {
        let case = format!("{key}={}", value.unwrap_or("<missing>"));
        match check_lock_case(key, value) {
            Ok((rejected_before_docker, outcome)) => {
                if !rejected_before_docker {
                    failures.push(case);
                }
                outcomes.push(outcome);
            }
            Err(error) => {
                failures.push(format!("{case}: fixture or CLI error: {error}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "invalid tool lock entries were not rejected before Docker: {failures:?}; outcomes:\n{}",
        outcomes.join("\n")
    );
}

fn check_lock_case(key: &str, value: Option<&str>) -> TestResult<(bool, String)> {
    let (git, source_sha) = fixture_with_lock_value(key, value)?;
    let (output, state) = qualify(&git, &source_sha, ApiPlan::new(&source_sha))?;
    let rejected_before_docker = !output.status.success()
        && state.api_requests == 0
        && state.image_inspections == 0
        && state.created.is_empty()
        && state.containers.is_empty()
        && state.volumes.is_empty()
        && state.volume_creations == 0
        && !state.fault_reached;
    let created_roles = state
        .created
        .iter()
        .map(|container| container.role.as_str())
        .collect::<Vec<_>>();
    let outcome = format!(
        "key={key} value={value:?} exit={:?} rejected_before_docker={rejected_before_docker} api_requests={} image_inspections={} created_roles={created_roles:?} remaining_containers={} volumes={:?} volume_creations={} fault_reached={}",
        output.status.code(),
        state.api_requests,
        state.image_inspections,
        state.containers.len(),
        state.volumes,
        state.volume_creations,
        state.fault_reached,
    );
    Ok((rejected_before_docker, outcome))
}

fn fixture_with_lock_value(key: &str, value: Option<&str>) -> TestResult<(GitFixture, String)> {
    let git = GitFixture::new()?;
    let lock_path = git.root().join(".velnor/runner.lock");
    let original = fs::read_to_string(&lock_path)?;
    let key_prefix = format!("{key} = ");
    let mut found = false;
    let mut lines = Vec::new();
    for line in original.lines() {
        if line.starts_with(&key_prefix) {
            found = true;
            if let Some(value) = value {
                lines.push(format!("{key} = {value:?}"));
            }
        } else {
            lines.push(line.to_owned());
        }
    }
    if !found {
        return Err(format!("runner lock has no {key} entry").into());
    }
    fs::write(lock_path, format!("{}\n", lines.join("\n")))?;
    run_git(git.root(), &["add", ".velnor/runner.lock"])?;
    run_git(
        git.root(),
        &["commit", "--quiet", "-m", "change runner version pin"],
    )?;
    let source_sha = String::from_utf8(run_git_output(
        git.root(),
        &["rev-parse", "--verify", "HEAD^{commit}"],
    )?)?
    .trim()
    .to_owned();
    Ok((git, source_sha))
}

fn run_git(root: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    run_git_output(root, args).map(drop)
}

fn run_git_output(root: &Path, args: &[&str]) -> Result<Vec<u8>, Box<dyn Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .args(args)
        .output()?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}
