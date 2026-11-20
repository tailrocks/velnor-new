// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Owns a spawned `install.sh` so that no outcome of a test leaves it behind.
//!
//! The cache-swap tests turn a cache entry into a FIFO, and a reader of a FIFO
//! with no writer blocks forever. The script runs in its own process group, so
//! killing the test does not reach it. Three layers keep it from outliving the
//! test: a deadline in `wait_within`, a `Drop` that kills the group and lets
//! every blocked reader see EOF, and a detached watchdog that kills the group
//! and removes the temp dir even when the test process itself is killed.

use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::setup_action::program;

const WATCHDOG: &str = "sleep \"$3\"; kill -KILL -- \"-$1\" 2>/dev/null; rm -rf \"$2\"";

#[derive(Debug)]
pub(crate) struct Running {
    child: Child,
    watchdog: Child,
    work: std::path::PathBuf,
}

impl Running {
    pub(crate) fn start(
        mut command: Command,
        work: &Path,
        hard_limit: Duration,
    ) -> Result<Self, Box<dyn Error>> {
        let child = command.process_group(0).spawn()?;
        let watchdog = Command::new(program("sh"))
            .args(["-c", WATCHDOG, "sh"])
            .arg(child.id().to_string())
            .arg(work)
            .arg(hard_limit.as_secs().to_string())
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self {
            child,
            watchdog,
            work: work.to_owned(),
        })
    }

    pub(crate) fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }

    /// The exit status, or `None` once the deadline passed and the group was killed.
    pub(crate) fn wait_within(&mut self, limit: Duration) -> io::Result<Option<ExitStatus>> {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                self.terminate();
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn terminate(&mut self) {
        kill_group(self.child.id());
        release_fifos(&self.work);
        self.child.wait().ok();
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.terminate();
        kill_group(self.watchdog.id());
        self.watchdog.wait().ok();
    }
}

fn kill_group(leader: u32) {
    Command::new(program("sh"))
        .args(["-c", "kill -KILL -- \"-$1\" 2>/dev/null", "sh"])
        .arg(leader.to_string())
        .status()
        .ok();
}

/// Opens and closes the writer side of every FIFO under `dir`, so a reader that
/// is still blocked on one reads EOF instead of waiting forever.
fn release_fifos(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_fifo() => {
                fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .ok();
            }
            Ok(kind) if kind.is_dir() => release_fifos(&path),
            _ => {}
        }
    }
}
