// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use velnor_runner::exec::{AdapterRegistry, JobCancel, JobResult, run_job};

use crate::exec_builder::{Recorder, assert_gone, execute, job, parse, pid_from, run};
use crate::exec_support::Scratch;

const PRINT_ID: &str = "echo \"tracking=${RUNNER_TRACKING_ID-unset}\"";

fn tracking_ids(lines: &[String]) -> Vec<&str> {
    lines
        .iter()
        .filter_map(|line| line.strip_prefix("tracking="))
        .collect()
}

#[tokio::test]
async fn every_step_of_a_job_carries_the_same_tracking_id() -> Result<()> {
    let message = job(vec![run("a", PRINT_ID), run("b", PRINT_ID)]);

    let (ran, lines) = execute(
        &message,
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;

    assert_eq!(ran.result, JobResult::Succeeded);
    let ids = tracking_ids(&lines);
    assert_eq!(ids.len(), 2, "{lines:?}");
    assert!(ids[0].starts_with("github_"), "{ids:?}");
    assert_eq!(ids[0], ids[1]);
    Ok(())
}

#[tokio::test]
async fn a_job_whose_process_clean_variable_is_false_sets_no_tracking_id() -> Result<()> {
    let mut message = job(vec![run("a", PRINT_ID)]);
    message["Variables"] = json!({"process.clean": {"Value": "False", "IsSecret": false}});

    let (_, lines) = execute(
        &message,
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;

    assert_eq!(tracking_ids(&lines), ["unset"], "{lines:?}");
    Ok(())
}

fn leaving_the_group(scratch: &Scratch) -> (Value, std::path::PathBuf) {
    let pid_file = scratch.work_dir().with_file_name("escaped.pid");
    let script = format!(
        "set -m\ntool=\"$RUNNER_TEMP/sleeper\"\ncp \"$(command -v sleep)\" \"$tool\"\ncodesign -s - --force \"$tool\" >/dev/null 2>&1 || true\n\"$tool\" 4321 >/dev/null 2>&1 &\necho $! > {}",
        pid_file.display()
    );
    (job(vec![run("escape", &script)]), pid_file)
}

#[tokio::test]
async fn a_process_that_left_the_step_group_is_killed_when_the_job_ends() -> Result<()> {
    let scratch = Scratch::new()?;
    let (message, pid_file) = leaving_the_group(&scratch);

    let (ran, _) = execute(
        &message,
        &scratch,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;

    assert_eq!(ran.result, JobResult::Succeeded);
    assert_gone(pid_from(&pid_file).context("the escaped process")?)
}

#[tokio::test]
async fn a_process_that_left_the_step_group_is_killed_when_the_job_future_is_dropped() -> Result<()>
{
    let scratch = Scratch::new()?;
    let (mut message, pid_file) = leaving_the_group(&scratch);
    message["Steps"]
        .as_array_mut()
        .context("steps")?
        .push(run("hold", "sleep 60"));
    let message = parse(&message)?;
    let options = scratch.options();
    let adapters = AdapterRegistry::empty();
    let mut sink = Recorder::default();
    let running = run_job(&message, &options, &adapters, &mut sink, JobCancel::new());
    let started = async {
        while !pid_file.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };

    tokio::select! {
        _ = running => bail!("the job ended before it was dropped"),
        () = started => {}
    }

    assert_gone(pid_from(&pid_file)?)
}
