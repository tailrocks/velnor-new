// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const ACCEPTED: &str = "empty secret";
const REFUSAL: &str = "Config is invalid.";

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/job-authority")
        .join(name)
}

fn identity() -> TestResult<(String, String)> {
    let text = std::fs::read_to_string(data("identity.txt"))?;
    let mut lines = text.lines();
    let source = lines.next().ok_or("no generator source sha")?;
    let artifact = lines.next().ok_or("no artifact sha256")?;
    Ok((source.to_owned(), artifact.to_owned()))
}

fn connect(state: &Path, inventory: &Path, source_sha: &str, artifact: &str) -> TestResult<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_velnor-host"))
        .arg("--state")
        .arg(state)
        .args(["connect", "--connection-id", "conn-inventory-1"])
        .args(["--repo", "owner/repo", "--repository-id", "42"])
        .arg("--job-authority-inventory")
        .arg(inventory)
        .args(["--job-authority-generator-source-sha", source_sha])
        .args(["--job-authority-generator-artifact-sha256", artifact])
        .args(["--job-authority-generation", "1"])
        .args(["--scale-set", "ubuntu-26.04-scale-set"])
        .args(["--scale-set-label", "velnor"])
        .args(["--scale-set-label", "ubuntu-26.04-scale-set"])
        .args(["--source-policy-mode", "branch-restricted"])
        .args(["--event", "pull_request", "--event", "push"])
        .args(["--event", "merge_group", "--branch", "main"])
        .args(["--pull-request-rule", "*:42:main:42:feature"])
        .args(["--platform", "linux/amd64", "--max-jobs", "1"])
        .args(["--docker-context", "orbstack"])
        .args(["--endpoint", "unix:///var/run/docker.sock"])
        .args(["--runner-cpu-millicores", "1000"])
        .args(["--runner-memory-bytes", "1073741824"])
        .args(["--dind-cpu-millicores", "1000"])
        .args(["--dind-memory-bytes", "1073741824"])
        .args(["--operational-memory-reserve-bytes", "1073741824"])
        .stdin(Stdio::null())
        .output()?)
}

#[test]
fn connect_accepts_generated_inventory() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let state = scratch.path().canonicalize()?;
    let generated = std::fs::read_to_string(data("inventory.json"))?;
    let (source_sha, artifact) = identity()?;

    let accepted = state.join("accepted.json");
    std::fs::write(&accepted, &generated)?;
    let output = connect(&state, &accepted, &source_sha, &artifact)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(ACCEPTED), "{stderr}");
    assert!(!stderr.contains(REFUSAL), "{stderr}");

    let (job_key, _) = generated
        .split_once("\"job_key\": \"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .ok_or("the inventory lists no job")?;
    let tampered = state.join("tampered.json");
    std::fs::write(
        &tampered,
        generated.replacen(job_key, &format!("{job_key}-edited"), 1),
    )?;
    let output = connect(&state, &tampered, &source_sha, &artifact)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(REFUSAL), "{stderr}");
    Ok(())
}
