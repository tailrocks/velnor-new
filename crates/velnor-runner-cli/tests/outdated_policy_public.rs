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

#[test]
fn connect_names_the_outdated_policy_and_the_fix() -> Result<(), Box<dyn Error>> {
    let state = tempfile::tempdir()?;
    std::fs::write(state.path().join("host.toml"), OUTDATED_HOST)?;
    let output = host(state.path())
        .args([
            "connect",
            "--connection-id",
            "lab",
            "--repo",
            "acme/widgets",
            "--repository-id",
            "7",
            "--scale-set",
            "lab",
            "--scale-set-label",
            "lab",
            "--source-policy-mode",
            "repository",
            "--event",
            "pull_request",
            "--platform",
            "linux/arm64",
            "--runner-cpu-millicores",
            "2000",
            "--runner-memory-bytes",
            "4294967296",
            "--dind-cpu-millicores",
            "1000",
            "--dind-memory-bytes",
            "2147483648",
            "--operational-memory-reserve-bytes",
            "1073741824",
            "--endpoint",
            "unix:///fixture.sock",
        ])
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
