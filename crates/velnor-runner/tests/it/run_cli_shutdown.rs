// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use rustix::process::Signal;

use super::exec_builder::BACKGROUND_SLEEP;
use super::exec_support::Scratch;
use super::lifecycle_support::{fixture_job, job_replacing};
use super::run_cli::{signal_and_wait, start_runner, until};
use super::support::mock_github::{Behavior, MockGithub};

const DIRECTORIES: usize = 250;
const FILES_PER_DIRECTORY: usize = 1_000;
const EXIT_WITHIN: Duration = Duration::from_secs(3);
const CREATORS: usize = 8;

fn leave_workspace(workspace: &Path) -> Result<()> {
    std::thread::scope(|scope| {
        let creators: Vec<_> = (0..CREATORS)
            .map(|creator| {
                scope.spawn(move || -> Result<()> {
                    for directory in (creator..DIRECTORIES).step_by(CREATORS) {
                        let path = workspace.join(format!("d{directory}"));
                        std::fs::create_dir_all(&path)?;
                        for file in 0..FILES_PER_DIRECTORY {
                            std::fs::File::create(path.join(format!("f{file}")))?;
                        }
                    }
                    Ok(())
                })
            })
            .collect();
        creators.into_iter().try_for_each(|creator| {
            creator
                .join()
                .map_err(|_| anyhow::anyhow!("a creator panicked"))?
        })
    })
}

fn entries(directory: &Path) -> usize {
    std::fs::read_dir(directory).map_or(0, Iterator::count)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_cli_exits_on_sigterm_without_waiting_for_a_leftover_workspace_to_be_removed()
-> Result<()> {
    let mock = MockGithub::start(fixture_job("hello", None)?, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let workspace = scratch.work_dir().join("job").join("workspace");
    leave_workspace(&workspace)?;
    let started = start_runner(&mock, &scratch)?;
    until("the removal started", || {
        Ok((entries(&workspace) < DIRECTORIES).then_some(()))
    })
    .await?;

    let signalled = Instant::now();
    let output = signal_and_wait(started, Signal::TERM).await?;

    let exited = signalled.elapsed();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        exited < EXIT_WITHIN,
        "exited {exited:?} after the signal, the removal was waited for"
    );
    let left = entries(&workspace);
    assert!(left > 0, "the removal had finished before the exit");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_cli_clears_the_temp_directory_on_shutdown_though_it_skips_the_workspace() -> Result<()>
{
    let job = job_replacing("cancel", Some(("sleep 60", BACKGROUND_SLEEP)))?;
    let mock = MockGithub::start(job, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let started = start_runner(&mock, &scratch)?;
    let work_dir = started.work_dir.clone();
    until("the step started", || {
        Ok(work_dir.join("temp").join("bg.pid").exists().then_some(()))
    })
    .await?;

    let output = signal_and_wait(started, Signal::TERM).await?;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(mock.complete_request()?["conclusion"], "canceled");
    assert!(
        !work_dir.join("temp").exists(),
        "the temp directory holds the file commands of the job"
    );
    assert!(
        work_dir.join("workspace").exists(),
        "the workspace cleanup is skipped on shutdown"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_cli_keeps_the_tool_cache_on_shutdown() -> Result<()> {
    let script = format!("echo kept > \"$RUNNER_TOOL_CACHE/marker\"\n{BACKGROUND_SLEEP}");
    let job = job_replacing("cancel", Some(("sleep 60", &script)))?;
    let mock = MockGithub::start(job, Behavior::default()).await?;
    let scratch = Scratch::new()?;
    let started = start_runner(&mock, &scratch)?;
    let work_dir = started.work_dir.clone();
    until("the step started", || {
        Ok(work_dir.join("temp").join("bg.pid").exists().then_some(()))
    })
    .await?;

    let output = signal_and_wait(started, Signal::TERM).await?;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!work_dir.join("temp").exists());
    assert_eq!(
        std::fs::read_to_string(work_dir.join("_tool").join("marker"))?,
        "kept\n"
    );
    Ok(())
}
