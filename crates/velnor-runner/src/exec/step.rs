// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use rustix::process::Pid;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::time::Instant;
use uuid::Uuid;

use super::cancel::{Groups, group_of, terminate};
use super::lines::split_lines;
use super::output::{LINE_LIMIT, Output};
use super::private;
use super::shell;
use super::timeout::{Timeout, interrupted, stop_result};
use super::{AnnotationLevel, ExecError, JobCancel, StepResult};

const DRAIN_GRACE: Duration = Duration::from_secs(2);
const LINE_BUFFER: usize = 64;
const DRAIN_LIMIT: usize = (LINE_BUFFER + 2) * LINE_LIMIT;
const READ_PIECE: usize = 64 << 10;
const READ_LIMIT: usize = LINE_LIMIT + READ_PIECE;
const CAPTURE_LIMIT: usize = 8 << 20;

pub(super) struct ProcessSpec<'a> {
    pub(super) program: &'a str,
    pub(super) args: &'a [String],
    pub(super) dir: &'a Path,
    pub(super) env: &'a [(String, String)],
}

pub(super) struct Limits<'a> {
    pub(super) timeout: Option<Timeout>,
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
    Interrupted,
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

struct Chunk {
    stderr: bool,
    bytes: Vec<u8>,
    overflow: bool,
}

impl Chunk {
    fn text(self) -> (String, bool) {
        (
            String::from_utf8_lossy(&self.bytes).into_owned(),
            self.overflow,
        )
    }
}

async fn read_lines<R>(reader: R, stderr: bool, whole_lines: bool, sender: mpsc::Sender<Chunk>)
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    let mut overflow = false;
    let mut first = true;
    loop {
        let mut piece = Vec::new();
        let read = (&mut reader)
            .take(READ_PIECE as u64)
            .read_until(b'\n', &mut piece)
            .await;
        if matches!(read, Ok(0) | Err(_)) {
            if !line.is_empty() || overflow {
                let bytes = std::mem::take(&mut line);
                let _ = sender
                    .send(Chunk {
                        stderr,
                        bytes,
                        overflow,
                    })
                    .await;
            }
            break;
        }
        if std::mem::take(&mut first) && piece.starts_with(&[0xEF, 0xBB, 0xBF]) {
            piece.drain(..3);
        }
        let chunk = if whole_lines {
            let ended = piece.last() == Some(&b'\n');
            let room = READ_LIMIT.saturating_sub(line.len());
            overflow |= piece.len() > room;
            line.extend_from_slice(&piece[..piece.len().min(room)]);
            if !ended {
                continue;
            }
            Chunk {
                stderr,
                bytes: std::mem::take(&mut line),
                overflow: std::mem::take(&mut overflow),
            }
        } else {
            Chunk {
                stderr,
                bytes: piece,
                overflow: false,
            }
        };
        if sender.send(chunk).await.is_err() {
            break;
        }
    }
}

async fn supervise<S>(
    child: &mut Child,
    group: Option<Pid>,
    lines: &mut mpsc::Receiver<Chunk>,
    stop: &mut S,
    mut on_chunk: impl FnMut(Chunk),
) -> Result<Ending, ExecError>
where
    S: Future<Output = Ending> + Unpin,
{
    let mut open = true;
    loop {
        tokio::select! {
            line = lines.recv(), if open => match line {
                Some(chunk) => on_chunk(chunk),
                None => open = false,
            },
            status = child.wait() => {
                return status
                    .map(Ending::Exited)
                    .map_err(|source| ExecError::io("wait for the step process", source));
            }
            stopped = &mut *stop => {
                terminate(child, group).await;
                return Ok(stopped);
            }
        }
    }
}

enum Drained {
    Closed,
    Cut,
    Stopped(Ending),
}

async fn drain<S>(
    lines: &mut mpsc::Receiver<Chunk>,
    stop: &mut S,
    watch: bool,
    mut on_chunk: impl FnMut(Chunk),
) -> Drained
where
    S: Future<Output = Ending> + Unpin,
{
    let mut waited = Duration::ZERO;
    let mut backlog = DRAIN_LIMIT;
    loop {
        let started = Instant::now();
        let received = tokio::select! {
            stopped = &mut *stop, if watch => return Drained::Stopped(stopped),
            received = tokio::time::timeout(DRAIN_GRACE.saturating_sub(waited), lines.recv()) => received,
        };
        waited += started.elapsed();
        match received {
            Ok(Some(chunk)) => {
                backlog = backlog.saturating_sub(chunk.bytes.len().max(1));
                on_chunk(chunk);
                if backlog == 0 {
                    return Drained::Cut;
                }
            }
            Ok(None) => return Drained::Closed,
            Err(_) => return Drained::Cut,
        }
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(1)
}

async fn drive(
    spec: &ProcessSpec<'_>,
    limits: &Limits<'_>,
    whole_lines: bool,
    mut on_chunk: impl FnMut(Chunk),
) -> Result<Ending, ExecError> {
    let mut child = spawn(spec).map_err(|source| {
        ExecError::io(
            format!("start {} in {}", spec.program, spec.dir.display()),
            source,
        )
    })?;
    limits.groups.track(&child);
    let group = group_of(&child);
    let (sender, mut lines) = mpsc::channel(LINE_BUFFER);
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(tokio::spawn(read_lines(
            stdout,
            false,
            whole_lines,
            sender.clone(),
        )));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(tokio::spawn(read_lines(
            stderr,
            true,
            whole_lines,
            sender.clone(),
        )));
    }
    drop(sender);
    let stop = async {
        interrupted(limits.cancel, limits.timeout).await;
        Ending::Interrupted
    };
    tokio::pin!(stop);
    let ending = supervise(&mut child, group, &mut lines, &mut stop, &mut on_chunk).await;
    let watch = matches!(ending, Ok(Ending::Exited(_)));
    let drained = drain(&mut lines, &mut stop, watch, on_chunk).await;
    if !matches!(drained, Drained::Closed) {
        for reader in &readers {
            reader.abort();
        }
    }
    match drained {
        Drained::Stopped(stopped) => {
            terminate(&mut child, group).await;
            Ok(stopped)
        }
        Drained::Closed | Drained::Cut => ending,
    }
}

pub(super) async fn run(
    spec: &ProcessSpec<'_>,
    limits: &Limits<'_>,
    out: &mut Output<'_>,
) -> Result<StepResult, ExecError> {
    let ending = drive(spec, limits, true, |chunk| {
        let (text, overflow) = chunk.text();
        let mut lines = split_lines(&text).peekable();
        while let Some(line) = lines.next() {
            out.bounded_line(line, overflow && lines.peek().is_none());
        }
    })
    .await?;
    Ok(match ending {
        Ending::Exited(status) if status.success() => StepResult::Succeeded,
        Ending::Exited(status) => {
            out.annotate(
                AnnotationLevel::Error,
                format!("Process completed with exit code {}.", exit_code(status)),
            );
            StepResult::Failed
        }
        Ending::Interrupted => stop_result(limits.cancel),
    })
}

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

impl Captured {
    fn push(&mut self, chunk: &[u8]) {
        let room = CAPTURE_LIMIT - self.bytes.len();
        self.truncated |= chunk.len() > room;
        self.bytes
            .extend_from_slice(&chunk[..chunk.len().min(room)]);
    }
}

pub(super) async fn capture(
    spec: &ProcessSpec<'_>,
    limits: &Limits<'_>,
) -> Result<String, ExecError> {
    let mut stdout = Captured::default();
    let mut stderr = Captured::default();
    let ending = drive(spec, limits, false, |chunk| {
        let target = if chunk.stderr {
            &mut stderr
        } else {
            &mut stdout
        };
        target.push(&chunk.bytes);
    })
    .await?;
    let command = || format!("{} {}", spec.program, spec.args.join(" "));
    match ending {
        Ending::Interrupted => Err(ExecError::Cancelled),
        Ending::Exited(status) if status.success() && stdout.truncated => Err(ExecError::Invalid(
            format!("{} printed more than {CAPTURE_LIMIT} bytes", command()),
        )),
        Ending::Exited(status) if status.success() => {
            Ok(String::from_utf8_lossy(&stdout.bytes).into_owned())
        }
        Ending::Exited(_) => Err(ExecError::Invalid(format!(
            "{} failed: {}",
            command(),
            String::from_utf8_lossy(&stderr.bytes).trim()
        ))),
    }
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
    private::write_new(&path, step.script.as_bytes()).await?;
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
