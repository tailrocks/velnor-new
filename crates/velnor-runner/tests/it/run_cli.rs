// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Output;

use anyhow::{Context, Result};
use tokio::process::Command;

use super::exec_support::{Scratch, fixture_path};
use super::support::mock_github::{Behavior, MockGithub};

async fn velnor_runner(arguments: &[&str]) -> Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_velnor-runner"))
        .args(arguments)
        .output()
        .await
        .context("run velnor-runner")
}

#[tokio::test]
async fn run_cli_rejects_invalid_jit_config() -> Result<()> {
    let output = velnor_runner(&["run", "--jit", "not-base64"]).await?;

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("decode encoded_jit_config"), "{stderr}");
    Ok(())
}

#[tokio::test]
async fn run_cli_requires_jit_argument() -> Result<()> {
    let output = velnor_runner(&["run"]).await?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--jit"));
    Ok(())
}

#[tokio::test]
async fn run_cli_runs_the_job_of_a_jit_file() -> Result<()> {
    let job = serde_json::from_str(&std::fs::read_to_string(fixture_path("hello"))?)?;
    let mock = MockGithub::start(job, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    std::fs::create_dir_all(scratch.work_dir())?;
    let jit_file = scratch.work_dir().join("jit.txt");
    std::fs::write(&jit_file, mock.jit())?;
    let jit_argument = format!("@{}", jit_file.display());
    let work_dir = scratch.work_dir().join("job");

    let output = velnor_runner(&[
        "run",
        "--jit",
        &jit_argument,
        "--work-dir",
        &work_dir.display().to_string(),
    ])
    .await?;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(mock.complete_request()?["conclusion"], "succeeded");
    Ok(())
}
