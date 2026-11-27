// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;
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
    let reader = child.stdout.take().map(|mut stdout| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stdout.read_to_end(&mut bytes);
            bytes
        })
    });
    let status = wait_until(&mut child, Instant::now() + deadline)?;
    Ok(Finished {
        status,
        stdout: collected(reader),
    })
}

fn wait_until(child: &mut Child, limit: Instant) -> Result<ExitStatus, RustRunnerFailure> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < limit => std::thread::sleep(POLL),
            Ok(None) | Err(_) => {
                end_group(child);
                return Err(RustRunnerFailure::OrbTimeout);
            }
        }
    }
}

fn end_group(child: &mut Child) {
    if let Some(group) = i32::try_from(child.id()).ok().and_then(Pid::from_raw) {
        let _ = kill_process_group(group, Signal::KILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn collected(reader: Option<JoinHandle<Vec<u8>>>) -> Vec<u8> {
    reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default()
}
