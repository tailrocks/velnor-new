// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::lifecycle_support::fixture_job;
use super::run_cli::velnor_runner;
use super::support::mock_github::{Behavior, MockGithub};

const EXIT_BUDGET: Duration = Duration::from_secs(20);

#[tokio::test]
async fn run_cli_refuses_an_inline_jit_value_and_does_not_repeat_it() -> Result<()> {
    let mock = MockGithub::start(fixture_job("hello", None)?, Behavior::default()).await?;

    let output = velnor_runner(&["run", "--jit", mock.jit()]).await?;

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("@FILE"), "{stderr}");
    assert!(!stderr.contains(mock.jit()), "the key was echoed");
    assert!(mock.server.requests()?.is_empty(), "the runner went on");
    Ok(())
}

#[tokio::test]
async fn run_cli_reads_the_jit_value_from_standard_input() -> Result<()> {
    let mock = MockGithub::start(fixture_job("hello", None)?, Behavior::default()).await?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_velnor-runner"))
        .args(["run", "--jit", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("start velnor-runner")?;
    let mut stdin = child.stdin.take().context("the runner stdin")?;
    stdin.write_all(mock.jit().as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    drop(stdin);

    let output = tokio::time::timeout(EXIT_BUDGET, child.wait_with_output())
        .await
        .context("the runner exited")??;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(mock.complete_request()?["conclusion"], "succeeded");
    Ok(())
}
