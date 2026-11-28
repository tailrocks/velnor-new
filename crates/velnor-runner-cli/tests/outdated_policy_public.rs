// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const ADVICE: &str = "regenerate the job-authority policy";
const OUTDATED_HOST: &str = "schema = 2\n[[job_authority_policies]]\nschema_version = 1\n";

fn host(state: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-host"));
    command.arg("--state").arg(state).env_remove("HOME");
    command
}

fn connect(state: &std::path::Path) -> Command {
    let mut command = host(state);
    command.args([
        "connect",
        "--connection-id",
        "conn-test-1",
        "--repo",
        "owner/repo",
        "--repository-id",
        "42",
        "--scale-set",
        "ubuntu-26.04-scale-set",
        "--scale-set-label",
        "velnor",
        "--scale-set-label",
        "ubuntu-26.04-scale-set",
        "--source-policy-mode",
        "branch-restricted",
        "--event",
        "push",
        "--branch",
        "main",
        "--platform",
        "linux/amd64",
        "--launch-mode",
        "official-runner",
        "--max-jobs",
        "1",
        "--docker-context",
        "orbstack",
        "--endpoint",
        "unix:///var/run/docker.sock",
        "--runner-cpu-millicores",
        "1000",
        "--runner-memory-bytes",
        "1073741824",
        "--dind-cpu-millicores",
        "1000",
        "--dind-memory-bytes",
        "1073741824",
        "--operational-memory-reserve-bytes",
        "1073741824",
    ]);
    command
}

#[test]
fn connect_names_the_outdated_policy_and_the_fix() -> Result<(), Box<dyn Error>> {
    let state = tempfile::tempdir()?;
    std::fs::write(state.path().join("host.toml"), OUTDATED_HOST)?;
    let output = connect(state.path()).stdin(Stdio::null()).output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(ADVICE), "{stderr}");
    assert!(stderr.contains("schema 1, expected 2"), "{stderr}");
    Ok(())
}

#[test]
fn connect_names_an_outdated_policy_file_and_the_fix() -> Result<(), Box<dyn Error>> {
    let state = tempfile::tempdir()?;
    let policy = state.path().join("policy.json");
    std::fs::write(&policy, "{\"schema_version\":1}")?;
    let output = connect(state.path())
        .arg("--job-authority-policy")
        .arg(&policy)
        .stdin(Stdio::null())
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(ADVICE), "{stderr}");
    assert!(stderr.contains("schema 1, expected 2"), "{stderr}");
    Ok(())
}

#[test]
fn the_daemon_names_the_outdated_policy_and_the_fix() -> Result<(), Box<dyn Error>> {
    let state = tempfile::tempdir()?;
    std::fs::write(state.path().join("host.toml"), OUTDATED_HOST)?;
    let mut child = host(state.path())
        .args(["daemon", "run"])
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = child.stderr.take().ok_or("stderr is not piped")?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut lines = BufReader::new(stderr).lines();
        let _ = sender.send(lines.next());
    });
    let first = receiver.recv_timeout(Duration::from_secs(60));
    child.kill()?;
    child.wait()?;
    let line = first?.ok_or("the daemon closed stderr")??;
    assert!(line.contains(ADVICE), "{line}");
    Ok(())
}
