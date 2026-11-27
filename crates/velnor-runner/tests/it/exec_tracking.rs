// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use velnor_runner::exec::{AdapterRegistry, JobCancel, JobResult, run_job};

use rustix::process::{Pid, Signal, kill_process};

use crate::exec_builder::{Recorder, assert_gone, execute, job, parse, pid_from, run, survives};
use crate::exec_support::Scratch;

const SURVIVAL: Duration = Duration::from_secs(1);
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

const COPY_SLEEP: &str = "set -m\nmkdir -p \"$RUNNER_TEMP/bin\"\ntool=\"$RUNNER_TEMP/bin/sleep\"\ncp \"$(command -v sleep)\" \"$tool\"\ncodesign -s - --force \"$tool\" >/dev/null 2>&1 || true\n";

fn pid_file_of(scratch: &Scratch, name: &str) -> PathBuf {
    scratch.work_dir().with_file_name(name)
}

fn leaving_the_group(scratch: &Scratch) -> (Value, PathBuf, PathBuf) {
    let pid_file = pid_file_of(scratch, "escaped.pid");
    let alive_file = pid_file_of(scratch, "escaped.alive");
    let script = format!(
        "{COPY_SLEEP}\"$tool\" 4321 >/dev/null 2>&1 &\npid=$!\necho \"$pid\" > {}\nsleep 0.3\nkill -0 \"$pid\" && touch {}",
        pid_file.display(),
        alive_file.display()
    );
    (job(vec![run("escape", &script)]), pid_file, alive_file)
}

#[tokio::test]
async fn a_process_that_left_the_step_group_is_killed_when_the_job_ends() -> Result<()> {
    let scratch = Scratch::new()?;
    let (message, pid_file, _) = leaving_the_group(&scratch);

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
    let (mut message, pid_file, alive_file) = leaving_the_group(&scratch);
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
        while !alive_file.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };

    tokio::select! {
        _ = running => bail!("the job ended before it was dropped"),
        () = started => {}
    }

    assert_gone(pid_from(&pid_file)?)
}

#[tokio::test]
async fn a_process_with_no_marker_or_another_jobs_marker_survives_the_job() -> Result<()> {
    let scratch = Scratch::new()?;
    let foreign = pid_file_of(&scratch, "foreign.pid");
    let unmarked = pid_file_of(&scratch, "unmarked.pid");
    let script = format!(
        "{COPY_SLEEP}RUNNER_TRACKING_ID=github_other \"$tool\" 4322 >/dev/null 2>&1 &\nforeign=$!\necho \"$foreign\" > {}\nenv -u RUNNER_TRACKING_ID \"$tool\" 4323 >/dev/null 2>&1 &\nunmarked=$!\necho \"$unmarked\" > {}\nsleep 0.3\nkill -0 \"$foreign\" && kill -0 \"$unmarked\"",
        foreign.display(),
        unmarked.display()
    );

    let outcome = execute(
        &job(vec![run("bystanders", &script)]),
        &scratch,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await;
    let pids = [pid_from(&foreign), pid_from(&unmarked)];
    let alive = pids
        .each_ref()
        .map(|pid| pid.as_ref().is_ok_and(|pid| survives(*pid, SURVIVAL)));
    for pid in pids.into_iter().flatten() {
        let _ = kill_process(pid, Signal::KILL);
    }

    assert_eq!(outcome?.0.result, JobResult::Succeeded);
    assert_eq!(alive, [true, true]);
    Ok(())
}

async fn run_leaving_a_child_in_the_step_group(clean: &str) -> Result<Pid> {
    let scratch = Scratch::new()?;
    let pid_file = pid_file_of(&scratch, "grouped.pid");
    let script = format!(
        "{COPY_SLEEP}set +m\n\"$tool\" 4324 >/dev/null 2>&1 &\npid=$!\necho \"$pid\" > {}\nsleep 0.3\nkill -0 \"$pid\"",
        pid_file.display()
    );
    let mut message = job(vec![run("grouped", &script)]);
    message["Variables"] = json!({"process.clean": {"Value": clean, "IsSecret": false}});

    let (ran, _) = execute(
        &message,
        &scratch,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;

    assert_eq!(ran.result, JobResult::Succeeded);
    pid_from(&pid_file).context("the child in the step group")
}

#[tokio::test]
async fn a_job_with_process_clean_false_leaves_a_child_in_the_step_group_running() -> Result<()> {
    let pid = run_leaving_a_child_in_the_step_group("False").await?;

    let alive = survives(pid, SURVIVAL);
    let _ = kill_process(pid, Signal::KILL);

    assert!(alive, "the job-end cleanup killed the child");
    Ok(())
}

#[tokio::test]
async fn a_job_with_process_clean_true_kills_a_child_in_the_step_group() -> Result<()> {
    let pid = run_leaving_a_child_in_the_step_group("True").await?;

    assert_gone(pid)
}
