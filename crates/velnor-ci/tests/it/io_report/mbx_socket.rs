// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use nix::sys::socket::{AddressFamily, SockFlag, SockType, UnixAddr, connect, socket};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const COMMAND_LIMIT: Duration = Duration::from_secs(60);
const BACKLOG_LIMIT: usize = 1 << 12;

#[derive(Clone, Copy)]
pub(super) enum Socket {
    Absent,
    Live,
    Stale,
    Busy,
}

pub(super) fn fill_backlog(path: &Path) -> Result<Vec<UnixStream>, Box<dyn std::error::Error>> {
    let address = UnixAddr::new(path)?;
    let mut clients = Vec::new();
    while clients.len() < BACKLOG_LIMIT {
        let client = UnixStream::from(socket(
            AddressFamily::Unix,
            SockType::Stream,
            SockFlag::empty(),
            None,
        )?);
        client.set_nonblocking(true)?;
        if connect(client.as_raw_fd(), &address).is_err() {
            break;
        }
        clients.push(client);
    }
    Ok(clients)
}

pub(super) fn run_bounded(command: &mut Command) -> Result<Output, Box<dyn std::error::Error>> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + COMMAND_LIMIT;
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            child.kill()?;
            return Err("velnor-ci did not finish within the limit".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(child.wait_with_output()?)
}
