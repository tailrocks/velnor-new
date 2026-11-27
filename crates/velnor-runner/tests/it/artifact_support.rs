// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration as StdDuration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use rustix::process::{Pid, Signal, kill_process};
use serde_json::{Value, json};
use time::{Date, Duration, Month, OffsetDateTime, PrimitiveDateTime, Time};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::exec_support::{Ran, Scratch, run_path_env};

const HANG_GUARD: StdDuration = StdDuration::from_secs(300);

pub(crate) const JOB_RUN: &str = "00000000-0000-0000-0000-000000000002";
pub(crate) const OLD_JOB: &str = "00000000-0000-0000-0000-0000000000a2";

pub(crate) fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jobs")
        .join(format!("{name}.json"))
}

fn escaped(literal: &str) -> String {
    literal
        .replace('\'', "''")
        .replace('{', "{{")
        .replace('}', "}}")
}

fn input_value(value: &str) -> Value {
    let (mut format, mut expressions, mut rest) = (String::new(), Vec::new(), value);
    while let Some(start) = rest.find("${{") {
        let Some(length) = rest[start..].find("}}") else {
            break;
        };
        format.push_str(&escaped(&rest[..start]));
        format.push('{');
        format.push_str(&expressions.len().to_string());
        format.push('}');
        expressions.push(rest[start + 3..start + length].trim());
        rest = &rest[start + length + 2..];
    }
    format.push_str(&escaped(rest));
    match expressions.as_slice() {
        [] => json!({ "type": 0, "lit": value }),
        [only] if format == "{0}" => json!({ "type": 3, "expr": only }),
        _ => json!({
            "type": 3,
            "expr": format!("format('{format}', {})", expressions.join(", ")),
        }),
    }
}

fn inputs_token(inputs: &[(&str, &str)]) -> Value {
    let map: Vec<Value> = inputs
        .iter()
        .map(
            |(key, value)| json!({ "key": { "type": 0, "lit": key }, "value": input_value(value) }),
        )
        .collect();
    json!({ "type": 2, "map": map })
}

pub(crate) fn run_job(
    scratch: &Scratch,
    name: &str,
    results_url: &str,
    inputs: &[(&str, &str)],
) -> Result<Ran> {
    run_job_env(scratch, name, results_url, inputs, Env::default())
}

#[derive(Default, Clone, Copy)]
pub(crate) struct Env<'a> {
    pub(crate) process: &'a [(&'a str, &'a str)],
    pub(crate) tz: Option<&'a std::ffi::OsStr>,
    pub(crate) job: &'a [(&'a str, &'a str)],
    pub(crate) revision: Option<&'a str>,
    pub(crate) script_before: Option<&'a str>,
}

fn write_job(
    scratch: &Scratch,
    name: &str,
    results_url: &str,
    inputs: &[(&str, &str)],
    env: Env<'_>,
) -> Result<PathBuf> {
    let body = std::fs::read_to_string(fixture(name)).context("read the fixture")?;
    let mut value: Value = serde_json::from_str(&body)?;
    value["Resources"]["Endpoints"][0]["Data"]["ResultsServiceUrl"] = json!(results_url);
    if !env.job.is_empty() {
        value["EnvironmentVariables"] = json!([inputs_token(env.job)]);
    }
    if !inputs.is_empty() {
        value["Steps"][0]["Inputs"] = inputs_token(inputs);
    }
    if let Some(revision) = env.revision {
        value["Steps"][0]["Reference"]["Ref"] = json!(revision);
    }
    if let Some(script) = env.script_before {
        let step = json!({
            "Type": "action",
            "Id": "00000000-0000-0000-0000-0000000000f1",
            "Name": "__run",
            "ContextName": "before",
            "Reference": { "Type": "script" },
            "DisplayNameToken": { "type": 0, "lit": "before" },
            "Inputs": inputs_token(&[("script", script)]),
        });
        value["Steps"]
            .as_array_mut()
            .context("the fixture has steps")?
            .insert(0, step);
    }
    let path = scratch.workspace().with_file_name("job.json");
    std::fs::create_dir_all(path.parent().context("job file has a parent")?)?;
    std::fs::write(&path, serde_json::to_vec(&value)?).context("write the job message")?;
    Ok(path)
}

pub(crate) fn run_job_env(
    scratch: &Scratch,
    name: &str,
    results_url: &str,
    inputs: &[(&str, &str)],
    env: Env<'_>,
) -> Result<Ran> {
    let path = write_job(scratch, name, results_url, inputs, env)?;
    run_path_env(&path, scratch, env.process, env.tz)
}

pub(crate) struct Running {
    child: Child,
    lines: mpsc::Receiver<String>,
    drain: Option<JoinHandle<String>>,
}

pub(crate) struct Finished {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
}

impl Running {
    pub(crate) fn wait_for_line(&self, needle: &str) -> Result<()> {
        let started = Instant::now();
        loop {
            let left = HANG_GUARD
                .checked_sub(started.elapsed())
                .ok_or_else(|| anyhow!("timed out waiting for the line {needle}"))?;
            let line = self
                .lines
                .recv_timeout(left)
                .map_err(|_| anyhow!("the runner never printed {needle}"))?;
            if line.contains(needle) {
                return Ok(());
            }
        }
    }

    pub(crate) fn pid(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn terminate(&self) -> Result<()> {
        let runner = Pid::from_raw(i32::try_from(self.child.id())?).context("the runner pid")?;
        kill_process(runner, Signal::TERM).context("terminate the runner")
    }

    fn reap(mut self) -> Result<Finished> {
        let code = self.child.wait().context("reap the runner")?.code();
        let stdout = self
            .drain
            .take()
            .context("the output is drained once")?
            .join()
            .map_err(|_| anyhow!("the output reader panicked"))?;
        Ok(Finished { code, stdout })
    }

    pub(crate) fn stop(mut self) -> Result<Finished> {
        self.child.kill().context("stop the runner")?;
        self.reap()
    }

    pub(crate) fn finish(mut self, what: &str) -> Result<Finished> {
        match wait_until(what, || matches!(self.child.try_wait(), Ok(Some(_)))) {
            Ok(()) => self.reap(),
            Err(error) => {
                self.stop()?;
                Err(error)
            }
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(crate) fn spawn_job(
    scratch: &Scratch,
    name: &str,
    results_url: &str,
    inputs: &[(&str, &str)],
    process: &[(&str, &str)],
) -> Result<Running> {
    let path = write_job(scratch, name, results_url, inputs, Env::default())?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_velnor-runner"))
        .envs(process.iter().copied())
        .arg("exec")
        .arg("--job")
        .arg(&path)
        .arg("--workspace")
        .arg(scratch.workspace())
        .arg("--temp")
        .arg(scratch.temp())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn velnor-runner exec")?;
    let stdout = child.stdout.take().context("the runner stdout")?;
    let (sender, lines) = mpsc::channel();
    let drain = std::thread::spawn(move || {
        let mut all = String::new();
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            all.push_str(&line);
            all.push('\n');
            let _ = sender.send(line);
        }
        all
    });
    Ok(Running {
        child,
        lines,
        drain: Some(drain),
    })
}

pub(crate) fn output(ran: &Ran, name: &str) -> Option<String> {
    let prefix = format!("output {name}=");
    ran.stdout
        .lines()
        .find_map(|line| line.strip_prefix(&prefix).map(str::to_owned))
}

pub(crate) fn read_file(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    let path = path.as_ref();
    std::fs::read(path)
        .with_context(|| format!("the artifact file {} was not written", path.display()))
}

pub(crate) fn write_file(scratch: &Scratch, relative: &str, bytes: &[u8]) -> Result<()> {
    let path = scratch.workspace().join(relative);
    std::fs::create_dir_all(path.parent().context("file has a parent")?)?;
    std::fs::write(path, bytes).context("write a workspace file")
}

pub(crate) fn zip_of(entries: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer.start_file(*name, SimpleFileOptions::default())?;
        writer.write_all(bytes)?;
    }
    Ok(writer.finish()?.into_inner())
}

pub(crate) fn entries_of(zip: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let mut archive = ZipArchive::new(Cursor::new(zip))?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index)?;
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes)?;
        entries.push((
            String::from_utf8_lossy(member.name_raw()).into_owned(),
            bytes,
        ));
    }
    entries.sort();
    Ok(entries)
}

pub(crate) fn binary_bytes() -> Vec<u8> {
    (0..=255_u8).cycle().take(70_000).collect()
}

pub(crate) fn expires_in(create: &Value, days: i64) -> Result<()> {
    let text = create["expires_at"]
        .as_str()
        .ok_or_else(|| anyhow!("CreateArtifact sent no expires_at"))?;
    let at = parse_utc(text)?;
    let drift = at - (OffsetDateTime::now_utc() + Duration::days(days));
    assert!(drift.abs() < Duration::minutes(2), "{text}");
    Ok(())
}

fn parse_utc(text: &str) -> Result<OffsetDateTime> {
    let number = |range: std::ops::Range<usize>| -> Result<u8> {
        Ok(text
            .get(range)
            .ok_or_else(|| anyhow!("short timestamp {text}"))?
            .parse()?)
    };
    let year: i32 = text
        .get(0..4)
        .ok_or_else(|| anyhow!("short timestamp {text}"))?
        .parse()?;
    let date = Date::from_calendar_date(year, Month::try_from(number(5..7)?)?, number(8..10)?)?;
    let clock = Time::from_hms(number(11..13)?, number(14..16)?, number(17..19)?)?;
    Ok(PrimitiveDateTime::new(date, clock).assume_utc())
}

pub(crate) fn wait_until(what: &str, mut ready: impl FnMut() -> bool) -> Result<()> {
    let started = Instant::now();
    while !ready() {
        if started.elapsed() > HANG_GUARD {
            bail!("timed out waiting for {what}");
        }
        std::thread::sleep(StdDuration::from_millis(2));
    }
    Ok(())
}

pub(crate) fn zip_stored(entries: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, bytes) in entries {
        writer.start_file(*name, options)?;
        writer.write_all(bytes)?;
    }
    Ok(writer.finish()?.into_inner())
}

pub(crate) fn error_lines(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|line| line.starts_with("::error::"))
        .collect()
}
