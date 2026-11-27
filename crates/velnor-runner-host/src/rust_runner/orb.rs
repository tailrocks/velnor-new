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

pub(super) struct Finished {
    pub(super) status: ExitStatus,
    pub(super) stdout: Vec<u8>,
}

pub(super) fn run(
    mut command: Command,
    deadline: Duration,
    capture: bool,
) -> Result<Finished, RustRunnerFailure> {
    command
        .stdin(Stdio::null())
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::inherit())
        .process_group(0);
    let mut child = command
        .spawn()
        .map_err(|_| RustRunnerFailure::OrbUnavailable)?;
    let limit = Instant::now() + deadline;
    let group = i32::try_from(child.id()).ok().and_then(Pid::from_raw);
    let reader = child.stdout.take().map(|mut stdout| {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stdout.read_to_end(&mut bytes);
            let _ = sender.send(bytes);
        });
        receiver
    });
    let status = wait_until(&mut child, group, limit)?;
    kill_group(group);
    Ok(Finished {
        status,
        stdout: collected(reader, group, limit)?,
    })
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

fn collected(
    reader: Option<Receiver<Vec<u8>>>,
    group: Option<Pid>,
    limit: Instant,
) -> Result<Vec<u8>, RustRunnerFailure> {
    let Some(reader) = reader else {
        return Ok(Vec::new());
    };
    reader
        .recv_timeout(limit.saturating_duration_since(Instant::now()))
        .map_err(|_| {
            kill_group(group);
            RustRunnerFailure::OrbTimeout
        })
}
