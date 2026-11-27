// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group};

use crate::error::RustRunnerFailure;

const POLL: Duration = Duration::from_millis(10);
const CAPTURE_LIMIT: u64 = 8 * 1024 * 1024;

pub(super) struct Finished {
    pub(super) status: ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
}

type Captured = Receiver<Option<Vec<u8>>>;

pub(super) fn run(
    command: Command,
    deadline: Duration,
    capture: bool,
) -> Result<Finished, RustRunnerFailure> {
    run_with_stdin(command, deadline, capture, Stdio::null())
}

pub(super) fn run_with_stdin(
    mut command: Command,
    deadline: Duration,
    capture: bool,
    stdin: Stdio,
) -> Result<Finished, RustRunnerFailure> {
    command
        .stdin(stdin)
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(if capture {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .process_group(0);
    let mut child = command
        .spawn()
        .map_err(|_| RustRunnerFailure::OrbUnavailable)?;
    let limit = Instant::now() + deadline;
    let group = i32::try_from(child.id()).ok().and_then(Pid::from_raw);
    let stdout = child.stdout.take().map(capture_of);
    let stderr = child.stderr.take().map(capture_of);
    let status = wait_until(&mut child, group, limit)?;
    kill_group(group);
    Ok(Finished {
        status,
        stdout: collected(stdout, group, limit)?,
        stderr: collected(stderr, group, limit)?,
    })
}

fn capture_of(stream: impl Read + Send + 'static) -> Captured {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(read_bounded(stream));
    });
    receiver
}

fn wait_until(
    child: &mut Child,
    group: Option<Pid>,
    limit: Instant,
) -> Result<ExitStatus, RustRunnerFailure> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < limit => std::thread::sleep(POLL),
            Ok(None) | Err(_) => {
                kill_group(group);
                let _ = child.kill();
                let _ = child.wait();
                return Err(RustRunnerFailure::OrbTimeout);
            }
        }
    }
}

fn kill_group(group: Option<Pid>) {
    if let Some(group) = group {
        let _ = kill_process_group(group, Signal::KILL);
    }
}

fn read_bounded(stdout: impl Read) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut bounded = stdout.take(CAPTURE_LIMIT + 1);
    let _ = bounded.read_to_end(&mut bytes);
    if u64::try_from(bytes.len()).is_ok_and(|read| read <= CAPTURE_LIMIT) {
        return Some(bytes);
    }
    let _ = std::io::copy(&mut bounded.into_inner(), &mut std::io::sink());
    None
}

fn collected(
    reader: Option<Captured>,
    group: Option<Pid>,
    limit: Instant,
) -> Result<Vec<u8>, RustRunnerFailure> {
    let Some(reader) = reader else {
        return Ok(Vec::new());
    };
    let received = reader
        .recv_timeout(limit.saturating_duration_since(Instant::now()))
        .map_err(|_| {
            kill_group(group);
            RustRunnerFailure::OrbTimeout
        })?;
    received.ok_or(RustRunnerFailure::OrbOutputTooLarge)
}
