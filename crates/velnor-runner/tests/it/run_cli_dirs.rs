// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use rustix::process::Signal;

use super::exec_builder::BACKGROUND_SLEEP;
use super::exec_support::Scratch;
use super::lifecycle_support::{fixture_job, job_replacing};
use super::run_cli::{signal_and_wait, start_runner_with, until};
use super::support::mock_github::{Behavior, MockGithub};

const EXIT_BUDGET: Duration = Duration::from_secs(20);

fn mode_of(path: &Path) -> Result<u32> {
    Ok(std::fs::metadata(path)
        .with_context(|| format!("stat {}", path.display()))?
        .permissions()
        .mode()
        & 0o777)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_cli_gives_the_work_directories_fixed_modes_whatever_the_umask() -> Result<()> {
    let job = job_replacing("cancel", Some(("sleep 60", BACKGROUND_SLEEP)))?;
    let mock = MockGithub::start(job, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let started = start_runner_with(&mock, &scratch, Some("000"), &[])?;
    let work_dir = started.work_dir.clone();
    until("the step started", || {
        Ok(work_dir.join("temp").join("bg.pid").exists().then_some(()))
    })
    .await?;

    let modes = [
        mode_of(&work_dir)?,
        mode_of(&work_dir.join("workspace"))?,
        mode_of(&work_dir.join("_tool"))?,
        mode_of(&work_dir.join("temp"))?,
        mode_of(&work_dir.join("logs"))?,
    ];
    signal_and_wait(started, Signal::TERM).await?;

    assert_eq!(
        modes,
        [0o755, 0o755, 0o755, 0o700, 0o700],
        "work, workspace, tool cache, temp and logs"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_cli_keeps_a_work_directory_that_exists_with_its_mode() -> Result<()> {
    let mock = MockGithub::start(fixture_job("hello", None)?, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let work_dir = scratch.work_dir().join("job");
    std::fs::create_dir_all(&work_dir)?;
    std::fs::set_permissions(&work_dir, std::fs::Permissions::from_mode(0o750))?;
    let cache = scratch.work_dir().join("hosted");
    let started = start_runner_with(
        &mock,
        &scratch,
        Some("000"),
        &[("RUNNER_TOOL_CACHE", cache.as_path())],
    )?;

    let output = tokio::time::timeout(EXIT_BUDGET, started.child.wait_with_output())
        .await
        .context("the runner exited")??;

    assert_eq!(output.status.code(), Some(0));
    assert!(
        work_dir.exists(),
        "the work directory of the operator stays"
    );
    assert_eq!(mode_of(&work_dir)?, 0o750);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_cli_uses_the_tool_cache_of_the_runner_environment() -> Result<()> {
    let script = "echo kept > \"$RUNNER_TOOL_CACHE/marker\"";
    let mock = MockGithub::start(fixture_job("hello", Some(script))?, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let cache = scratch.work_dir().join("hosted");
    let started = start_runner_with(
        &mock,
        &scratch,
        None,
        &[("RUNNER_TOOL_CACHE", cache.as_path())],
    )?;
    let work_dir = started.work_dir.clone();

    let output = tokio::time::timeout(EXIT_BUDGET, started.child.wait_with_output())
        .await
        .context("the runner exited")??;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_to_string(cache.join("marker"))?, "kept\n");
    assert!(!work_dir.join("_tool").exists());
    Ok(())
}
