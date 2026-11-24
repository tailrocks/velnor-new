// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::CiError;

const POLL: Duration = Duration::from_millis(20);
const CAPTURE_LIMIT: u64 = 1 << 20;

pub(super) fn run(command: &mut Command, timeout: Duration) -> Result<Vec<u8>, CiError> {
    let label = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| CiError::new(format!("cannot run {label}: {error}")))?;
    let mut pipe = child
        .stdout
        .take()
        .ok_or_else(|| CiError::new(format!("{label} has no output")))?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        (&mut pipe).take(CAPTURE_LIMIT).read_to_end(&mut bytes)?;
        io::copy(&mut pipe, &mut io::sink())?;
        Ok::<_, io::Error>(bytes)
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            Ok(None) => {
                drop(child.kill());
                drop(child.wait());
                return Err(CiError::new(format!(
                    "{label} did not finish within {} seconds.",
                    timeout.as_secs()
                )));
            }
            Err(error) => return Err(CiError::new(format!("cannot wait for {label}: {error}"))),
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| CiError::new(format!("cannot read the output of {label}")))?
        .map_err(|error| CiError::new(format!("cannot read the output of {label}: {error}")))?;
    if status.success() {
        Ok(bytes)
    } else {
        Err(CiError::new(format!("{label} failed: {status}.")))
    }
}

pub(super) fn text(command: &mut Command, timeout: Duration) -> Result<String, CiError> {
    let bytes = run(command, timeout)?;
    String::from_utf8(bytes).map_err(|_| CiError::new("command output is not UTF-8."))
}
