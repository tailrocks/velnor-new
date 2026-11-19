// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub struct Captured {
    pub success: bool,
    pub stdout: Vec<u8>,
}

pub fn capture(command: &mut Command, limit: u64, timeout: Duration) -> io::Result<Captured> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let pipe = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("no stdout"))?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            child.wait().ok();
            return Err(io::ErrorKind::TimedOut.into());
        }
        std::thread::sleep(POLL);
    };
    let stdout = reader
        .join()
        .map_err(|_| io::Error::other("reader panicked"))??;
    Ok(Captured {
        success: status.success(),
        stdout,
    })
}
