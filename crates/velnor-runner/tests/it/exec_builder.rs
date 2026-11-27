// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use rustix::process::{Pid, Signal, kill_process, test_kill_process};
use serde_json::{Value, json};
use velnor_runner::exec::{AdapterRegistry, JobCancel, JobRun, LogSink, StepResult, run_job};
use velnor_runner::job_message::AgentJobRequestMessage;

use crate::exec_support::{Ran, Scratch};

pub(crate) const HANG_GUARD: Duration = Duration::from_secs(300);

pub(crate) fn step(context: &str, script: Value) -> Value {
    let mut step = json!({
        "Type": "action",
        "Id": "00000000-0000-0000-0000-000000000001",
        "Name": context,
        "ContextName": context,
        "Reference": {"Type": "script"},
        "DisplayNameToken": {"type": 0, "lit": context},
        "Inputs": {"type": 2, "map": [{"key": {"type": 0, "lit": "script"}, "value": null}]}
    });
    step["Inputs"]["map"][0]["value"] = script;
    step
}

pub(crate) fn literal(text: &str) -> Value {
    json!({"type": 0, "lit": text})
}

pub(crate) fn expression(text: &str) -> Value {
    json!({"type": 3, "expr": text})
}

pub(crate) fn run(context: &str, script: &str) -> Value {
    step(context, literal(script))
}

pub(crate) fn with(mut step: Value, key: &str, value: Value) -> Value {
    step[key] = value;
    step
}

pub(crate) fn mapping(pairs: &[(&str, Value)]) -> Value {
    let map: Vec<Value> = pairs
        .iter()
        .map(|(key, value)| json!({"key": literal(key), "value": value}))
        .collect();
    json!({"type": 2, "map": map})
}

pub(crate) fn job(steps: Vec<Value>) -> Value {
    let mut message = json!({
        "MessageType": "PipelineAgentJobRequest",
        "JobId": "00000000-0000-0000-0000-000000000001",
        "JobDisplayName": "exec",
        "JobName": "exec",
        "RequestId": 1,
        "Steps": null
    });
    message["Steps"] = Value::Array(steps);
    message
}

pub(crate) fn write_job(scratch: &Scratch, message: &Value) -> Result<PathBuf> {
    let path = scratch.options().temp.with_extension("job.json");
    std::fs::write(&path, serde_json::to_string(message)?).context("write the job message")?;
    Ok(path)
}

pub(crate) fn binary(job: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-runner"));
    command.arg("exec").arg("--job").arg(job);
    command
}

pub(crate) fn bounded(scratch: &Scratch, job: &Path) -> Command {
    let options = scratch.options();
    let mut command = binary(job);
    command
        .arg("--workspace")
        .arg(options.workspace)
        .arg("--temp")
        .arg(options.temp);
    command
}

pub(crate) fn finished(mut command: Command) -> Result<Ran> {
    let started = Instant::now();
    let output = command.output().context("run velnor-runner exec")?;
    Ok(Ran {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code(),
        elapsed: started.elapsed(),
    })
}

fn drained(pipe: Option<impl std::io::Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

pub(crate) fn finished_within(mut command: Command, limit: Duration) -> Result<Ran> {
    let started = Instant::now();
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("start velnor-runner exec")?;
    let stdout = drained(child.stdout.take());
    let stderr = drained(child.stderr.take());
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > limit {
            child.kill().context("stop the stuck runner")?;
            child.wait().context("reap the stuck runner")?;
            bail!("velnor-runner exec did not finish within {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let collect = |handle: std::thread::JoinHandle<Vec<u8>>| {
        String::from_utf8_lossy(&handle.join().unwrap_or_default()).into_owned()
    };
    Ok(Ran {
        stdout: collect(stdout),
        stderr: collect(stderr),
        code: status.code(),
        elapsed: started.elapsed(),
    })
}

pub(crate) fn run_message(scratch: &Scratch, message: &Value) -> Result<Ran> {
    finished_within(bounded(scratch, &write_job(scratch, message)?), HANG_GUARD)
}

pub(crate) fn pid_from(path: &Path) -> Result<Pid> {
    let text = std::fs::read_to_string(path).context("read the recorded pid")?;
    let raw: i32 = text.trim().parse().context("parse the recorded pid")?;
    Pid::from_raw(raw).context("a valid pid")
}

pub(crate) fn wait_for_file(path: &Path) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        if Instant::now() > deadline {
            bail!("{} never appeared", path.display());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(50));
    Ok(())
}

pub(crate) fn assert_gone(pid: Pid) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while test_kill_process(pid).is_ok() {
        if Instant::now() > deadline {
            let _ = kill_process(pid, Signal::KILL);
            bail!("process {} is still alive", pid.as_raw_nonzero());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

pub(crate) const BACKGROUND_SLEEP: &str =
    "sleep 4242 >/dev/null 2>&1 &\necho $! > \"$RUNNER_TEMP/bg.pid\"\nwait";

pub(crate) const BACKGROUND_STATE: &str = "pid=$(cat \"$RUNNER_TEMP/bg.pid\")\nfor _ in $(seq 50); do\n  kill -0 \"$pid\" 2>/dev/null || { echo bg-dead; exit 0; }\n  sleep 0.1\ndone\necho bg-alive";

#[derive(Default)]
pub(crate) struct Recorder {
    pub(crate) lines: Vec<String>,
}

impl LogSink for Recorder {
    fn step_started(&mut self, _number: u32, _name: &str) {}

    fn line(&mut self, _number: u32, text: &str) {
        self.lines.push(text.to_owned());
    }

    fn step_finished(&mut self, _number: u32, _result: StepResult) {}

    fn job_finished(&mut self, _run: &JobRun) {}
}

pub(crate) fn parse(message: &Value) -> Result<AgentJobRequestMessage> {
    AgentJobRequestMessage::parse_json(&serde_json::to_string(message)?)
}

pub(crate) async fn execute(
    message: &Value,
    scratch: &Scratch,
    adapters: &AdapterRegistry,
    cancel: JobCancel,
) -> Result<(JobRun, Vec<String>)> {
    let mut recorder = Recorder::default();
    let run = tokio::time::timeout(
        HANG_GUARD,
        run_job(
            &parse(message)?,
            &scratch.options(),
            adapters,
            &mut recorder,
            cancel,
        ),
    )
    .await
    .context("run_job returns")?;
    Ok((run, recorder.lines))
}
