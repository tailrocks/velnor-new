// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use serde_json::json;
use velnor_runner::exec::{AdapterRegistry, JobCancel, JobResult, StepResult};

use crate::exec_builder::{
    HANG_GUARD, bounded, execute, finished_within, job, run, with, write_job,
};
use crate::exec_support::Scratch;

const WRITER_AFTER_EXIT: &str = "(while true; do echo tick; sleep 1; done) &\necho started";

#[tokio::test]
async fn a_background_writer_that_ticks_cannot_hold_a_finished_step_open() -> Result<()> {
    let (run, lines) = execute(
        &job(vec![run("a", WRITER_AFTER_EXIT)]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await
    .context("a step whose leader exited ends although a descendant keeps writing")?;
    assert_eq!(run.result, JobResult::Succeeded);
    assert_eq!(run.steps[0].outcome, StepResult::Succeeded);
    assert!(lines.iter().any(|line| line == "started"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn the_step_timeout_ends_the_drain_of_a_finished_step() -> Result<()> {
    let (run, lines) = execute(
        &job(vec![with(
            run("a", WRITER_AFTER_EXIT),
            "TimeoutInMinutes",
            json!(0.005),
        )]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(run.steps[0].outcome, StepResult::Failed);
    assert!(
        lines
            .iter()
            .any(|line| line == "step 1 timed out after 0.005 minutes"),
        "{lines:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_background_flood_is_cut_at_the_drain_limit() -> Result<()> {
    let script = format!("(yes {} &)\necho started", "x".repeat(1000));
    let (run, lines) = execute(
        &job(vec![run("a", &script)]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await
    .context("a step whose leader exited ends although a descendant floods its output")?;
    assert_eq!(run.result, JobResult::Succeeded);
    assert!(lines.iter().any(|line| line == "started"), "{lines:?}");
    let read: usize = lines.iter().map(String::len).sum();
    assert!(
        read < 80 << 20,
        "the drain read {read} bytes after the leader exited"
    );
    Ok(())
}

fn failing_file_command(setup: &str, expected: &str) -> Result<()> {
    let scratch = Scratch::new()?;
    let path = write_job(&scratch, &job(vec![run("a", setup)]))?;
    let ran = finished_within(bounded(&scratch, &path), HANG_GUARD)?;
    assert!(
        ran.has_line("Unable to process file command 'env' successfully."),
        "{}",
        ran.stdout
    );
    assert!(
        ran.stdout.lines().any(|line| line.ends_with(expected)),
        "{}",
        ran.stdout
    );
    assert!(ran.has_line("step 1 failed"), "{}", ran.stdout);
    assert_eq!(ran.code, Some(1));
    Ok(())
}

#[test]
fn a_github_env_that_links_to_a_device_fails_the_step_without_reading_it() -> Result<()> {
    failing_file_command("ln -sf /dev/zero \"$GITHUB_ENV\"", "not a regular file")
}

#[test]
fn a_github_env_that_is_a_fifo_fails_the_step_instead_of_blocking() -> Result<()> {
    failing_file_command(
        "rm -f \"$GITHUB_ENV\"\nmkfifo \"$GITHUB_ENV\"",
        "not a regular file",
    )
}

#[test]
fn a_file_command_file_over_the_limit_fails_the_step() -> Result<()> {
    let scratch = Scratch::new()?;
    let fill = |name: &str| format!("head -c 1100000 /dev/zero | tr '\\0' a >> \"${name}\"");
    let script = ["GITHUB_ENV", "GITHUB_OUTPUT", "GITHUB_PATH"]
        .map(fill)
        .join("\n");
    let path = write_job(&scratch, &job(vec![run("a", &script)]))?;
    let ran = finished_within(bounded(&scratch, &path), HANG_GUARD)?;
    for context in ["env", "output", "path"] {
        let header = format!("Unable to process file command '{context}' successfully.");
        assert!(ran.has_line(&header), "{}", ran.stdout);
    }
    let oversize = ran
        .stdout
        .lines()
        .filter(|line| line.ends_with("larger than 1048576 bytes"))
        .count();
    assert_eq!(oversize, 3, "{}", ran.stdout);
    assert_eq!(ran.code, Some(1));
    Ok(())
}
