// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::cancel::{Groups, terminate};
use super::commands;
use super::mask::Masker;
use super::shell;
use super::sink::LogSink;
use super::{Annotation, AnnotationLevel, ExecError, JobCancel, StepResult};

const DRAIN_GRACE: Duration = Duration::from_secs(2);
const LINE_BUFFER: usize = 256;

pub(super) struct Output<'a> {
    pub(super) number: u32,
    pub(super) sink: &'a mut dyn LogSink,
    pub(super) masker: &'a mut Masker,
    pub(super) annotations: &'a mut Vec<Annotation>,
}

impl Output<'_> {
    pub(super) fn line(&mut self, text: &str) {
        let before = self.annotations.len();
        commands::apply(text, self.masker, self.annotations);
        for annotation in &mut self.annotations[before..] {
            annotation.message = self.masker.mask(&annotation.message).into_owned();
        }
        self.message(text);
    }

    pub(super) fn message(&mut self, text: &str) {
        let masked = self.masker.mask(text);
        self.sink.line(self.number, &masked);
    }
}

pub(super) struct ProcessSpec<'a> {
    pub(super) program: &'a str,
    pub(super) args: &'a [String],
    pub(super) dir: &'a Path,
    pub(super) env: &'a [(String, String)],
}

pub(super) struct Limits<'a> {
    pub(super) minutes: Option<f64>,
    pub(super) cancel: Option<&'a JobCancel>,
    pub(super) groups: &'a Groups,
}

pub(super) struct ScriptStep {
    pub(super) script: String,
    pub(super) shell: String,
    pub(super) dir: PathBuf,
    pub(super) env: Vec<(String, String)>,
}

enum Ending {
    Exited(ExitStatus),
    TimedOut,
    Cancelled,
}

fn spawn(spec: &ProcessSpec<'_>) -> std::io::Result<Child> {
    Command::new(spec.program)
        .args(spec.args)
        .current_dir(spec.dir)
        .env_clear()
        .envs(spec.env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
}

async fn read_lines<R>(reader: R, sender: mpsc::Sender<String>)
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if buffer.last() == Some(&b'\n') {
            buffer.pop();
        }
        if buffer.last() == Some(&b'\r') {
            buffer.pop();
        }
        if sender
            .send(String::from_utf8_lossy(&buffer).into_owned())
            .await
            .is_err()
        {
            break;
        }
    }
}

async fn supervise(
    child: &mut Child,
    lines: &mut mpsc::Receiver<String>,
    limits: &Limits<'_>,
    out: &mut Output<'_>,
) -> Result<Ending, ExecError> {
    let deadline = deadline(limits.minutes);
    let timer = async {
        match deadline {
            Some(duration) => tokio::time::sleep(duration).await,
            None => std::future::pending().await,
        }
    };
    let cancelled = async {
        match limits.cancel {
            Some(cancel) => cancel.cancelled().await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(timer, cancelled);
    let mut open = true;
    loop {
        tokio::select! {
            line = lines.recv(), if open => match line {
                Some(text) => out.line(&text),
                None => open = false,
            },
            status = child.wait() => {
                return status
                    .map(Ending::Exited)
                    .map_err(|source| ExecError::io("wait for the step process", source));
            }
            () = &mut timer => {
                terminate(child).await;
                return Ok(Ending::TimedOut);
            }
            () = &mut cancelled => {
                terminate(child).await;
                return Ok(Ending::Cancelled);
            }
        }
    }
}

pub(super) fn deadline(minutes: Option<f64>) -> Option<Duration> {
    minutes.and_then(|minutes| Duration::try_from_secs_f64(minutes * 60.0).ok())
}

pub(super) fn report_timeout(out: &mut Output<'_>, minutes: Option<f64>) {
    let minutes = minutes.unwrap_or_default();
    out.message(&format!(
        "step {} timed out after {minutes} minutes",
        out.number
    ));
}

fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(1)
}

pub(super) async fn run(
    spec: &ProcessSpec<'_>,
    limits: &Limits<'_>,
    out: &mut Output<'_>,
) -> Result<StepResult, ExecError> {
    let mut child = spawn(spec).map_err(|source| {
        ExecError::io(
            format!("start {} in {}", spec.program, spec.dir.display()),
            source,
        )
    })?;
    limits.groups.track(&child);
    let (sender, mut lines) = mpsc::channel(LINE_BUFFER);
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(tokio::spawn(read_lines(stdout, sender.clone())));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(tokio::spawn(read_lines(stderr, sender.clone())));
    }
    drop(sender);
    let ending = supervise(&mut child, &mut lines, limits, out).await;
    let drained = tokio::time::timeout(DRAIN_GRACE, async {
        while let Some(text) = lines.recv().await {
            out.line(&text);
        }
    })
    .await;
    if drained.is_err() {
        for reader in &readers {
            reader.abort();
        }
    }
    Ok(match ending? {
        Ending::Exited(status) if status.success() => StepResult::Succeeded,
        Ending::Exited(status) => {
            out.annotations.push(Annotation::new(
                AnnotationLevel::Error,
                format!("Process completed with exit code {}.", exit_code(status)),
            ));
            StepResult::Failed
        }
        Ending::TimedOut => {
            report_timeout(out, limits.minutes);
            StepResult::Failed
        }
        Ending::Cancelled => StepResult::Cancelled,
    })
}

pub(super) async fn run_script(
    step: &ScriptStep,
    temp: &Path,
    limits: &Limits<'_>,
    out: &mut Output<'_>,
) -> Result<StepResult, ExecError> {
    if !shell::is_supported(&step.shell) {
        return Err(ExecError::Unsupported(format!("shell {}", step.shell)));
    }
    let path = temp.join(format!("{}.sh", Uuid::new_v4()));
    tokio::fs::write(&path, &step.script)
        .await
        .map_err(|source| ExecError::io(format!("write {}", path.display()), source))?;
    let Some((program, args)) = shell::command(&step.shell, &path) else {
        return Err(ExecError::Unsupported(format!("shell {}", step.shell)));
    };
    let spec = ProcessSpec {
        program: &program,
        args: &args,
        dir: &step.dir,
        env: &step.env,
    };
    let result = run(&spec, limits, out).await;
    let _ = tokio::fs::remove_file(&path).await;
    result
}
