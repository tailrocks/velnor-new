// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

use crate::error::CiError;
use crate::image_candidate::build::forwards;

const POLL: Duration = Duration::from_millis(20);
const CAPTURE_LIMIT: u64 = 1 << 20;

pub(super) fn clean_environment(command: &mut Command) -> &mut Command {
    command
        .env_clear()
        .envs(std::env::vars_os().filter(|(name, _)| name.to_str().is_some_and(forwards)))
}

fn end_group(child: &Child) {
    if let Ok(id) = i32::try_from(child.id()) {
        let _ = killpg(Pid::from_raw(id), Signal::SIGKILL);
    }
}

fn start(command: &mut Command, capture: bool, label: &str) -> Result<Child, CiError> {
    command
        .stdin(Stdio::null())
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .process_group(0)
        .spawn()
        .map_err(|error| CiError::new(format!("cannot run {label}: {error}")))
}

fn execute(command: &mut Command, timeout: Duration, capture: bool) -> Result<Vec<u8>, CiError> {
    let label = command.get_program().to_string_lossy().into_owned();
    let mut child = start(command, capture, &label)?;
    let reader = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            (&mut pipe).take(CAPTURE_LIMIT).read_to_end(&mut bytes)?;
            io::copy(&mut pipe, &mut io::sink())?;
            Ok::<_, io::Error>(bytes)
        })
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            Ok(None) => {
                end_group(&child);
                drop(child.wait());
                return Err(CiError::new(format!(
                    "{label} did not finish within {} seconds.",
                    timeout.as_secs()
                )));
            }
            Err(error) => {
                end_group(&child);
                return Err(CiError::new(format!("cannot wait for {label}: {error}")));
            }
        }
    };
    end_group(&child);
    let bytes = match reader {
        Some(thread) => thread
            .join()
            .map_err(|_| CiError::new(format!("cannot read the output of {label}")))?
            .map_err(|error| CiError::new(format!("cannot read the output of {label}: {error}")))?,
        None => Vec::new(),
    };
    if status.success() {
        Ok(bytes)
    } else {
        Err(CiError::new(format!("{label} failed: {status}.")))
    }
}

pub(super) fn run(command: &mut Command, timeout: Duration) -> Result<(), CiError> {
    execute(command, timeout, false).map(drop)
}

pub(super) fn text(command: &mut Command, timeout: Duration) -> Result<String, CiError> {
    let bytes = execute(command, timeout, true)?;
    String::from_utf8(bytes).map_err(|_| CiError::new("command output is not UTF-8."))
}
