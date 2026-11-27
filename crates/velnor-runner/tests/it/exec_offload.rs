// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rustix::process::{Pid, Signal, kill_process};
use serde_json::json;

use crate::exec_builder::{bounded, expression, job, run, step, wait_for_file, with, write_job};
use crate::exec_support::Scratch;

const SPARSE_BYTES: u64 = 16 << 30;
const FORCED_EXIT: i32 = 130;
const SIGNAL_GAP: Duration = Duration::from_millis(300);
const RESPONSE_BOUND: Duration = Duration::from_secs(2);

#[test]
fn signals_are_served_while_hash_files_evaluates_in_the_step_executor() -> Result<()> {
    let scratch = Scratch::new()?;
    std::fs::create_dir_all(scratch.workspace()).context("create the workspace")?;
    std::fs::File::create(scratch.workspace().join("big.bin"))
        .and_then(|file| file.set_len(SPARSE_BYTES))
        .context("create the large sparse file")?;
    let guarded = with(
        run("a", "echo first"),
        "Condition",
        json!("hashFiles('big.bin') != 'none'"),
    );
    let hashed = step("b", expression("format('echo {0}', hashFiles('big.bin'))"));
    let ready = run("r", "touch \"$RUNNER_TEMP/ready\"");
    let path = write_job(&scratch, &job(vec![ready, guarded, hashed]))?;
    let child = bounded(&scratch, &path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("start the runner")?;
    let runner = Pid::from_raw(i32::try_from(child.id())?).context("the runner pid")?;
    wait_for_file(&scratch.options().temp.join("ready"))?;
    std::thread::sleep(SIGNAL_GAP);
    kill_process(runner, Signal::TERM).context("send the first signal")?;
    std::thread::sleep(SIGNAL_GAP);
    let second = Instant::now();
    kill_process(runner, Signal::TERM).context("send the second signal")?;
    let output = child.wait_with_output().context("wait for the runner")?;
    assert_eq!(output.status.code(), Some(FORCED_EXIT));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("terminated by a second signal"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        second.elapsed() < RESPONSE_BOUND,
        "the runner took {:?} to serve the second signal while hashing",
        second.elapsed()
    );
    Ok(())
}
