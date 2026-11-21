// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;
use nix::errno::Errno;
use nix::sys::socket::{AddressFamily, SockFlag, SockType, UnixAddr, connect, socket};
use std::fs;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(super) struct SocketDir(PathBuf);

impl SocketDir {
    pub(super) fn for_repo(repo: &Repo) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let dir = PathBuf::from("/tmp").join(format!(
            "velnor-sock-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir)?;
        symlink(repo.root(), dir.join("r"))?;
        Ok(Self(dir))
    }

    pub(super) fn socket(&self, name: &str) -> PathBuf {
        self.0.join("r").join(name)
    }
}

impl Drop for SocketDir {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

const COMMAND_LIMIT: Duration = Duration::from_secs(60);
const BACKLOG_LIMIT: usize = 1 << 12;

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
        match connect(client.as_raw_fd(), &address) {
            Ok(()) => clients.push(client),
            Err(Errno::EINPROGRESS) => {
                clients.push(client);
                break;
            }
            Err(_) => break,
        }
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
