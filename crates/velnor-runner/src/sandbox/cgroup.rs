// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use rustix::process::{Pid, Signal, kill_process};

const POLL: Duration = Duration::from_millis(5);
const EMPTY_WITHIN: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub(super) struct Cgroup {
    path: PathBuf,
}

fn populated(path: &Path) -> Result<bool> {
    match std::fs::read_to_string(path.join("cgroup.events")) {
        Ok(events) => Ok(events.lines().any(|line| line == "populated 1")),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("read the events of {}", path.display())),
    }
}

fn kill_and_remove(path: &Path) -> Result<()> {
    match std::fs::write(path.join("cgroup.kill"), "1") {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("kill the processes of {}", path.display()));
        }
    }
    let deadline = Instant::now() + EMPTY_WITHIN;
    while populated(path)? {
        if Instant::now() > deadline {
            bail!("{} still holds processes after cgroup.kill", path.display());
        }
        std::thread::sleep(POLL);
    }
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

impl Cgroup {
    pub(super) fn create(path: PathBuf) -> Result<Self> {
        kill_and_remove(&path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        std::fs::create_dir(&path).with_context(|| format!("create {}", path.display()))?;
        Ok(Self { path })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn terminate(&self) {
        let Ok(members) = std::fs::read_to_string(self.path.join("cgroup.procs")) else {
            return;
        };
        for pid in members
            .lines()
            .filter_map(|line| line.parse::<i32>().ok())
            .filter_map(Pid::from_raw)
        {
            let _ = kill_process(pid, Signal::TERM);
        }
    }

    pub(super) fn kill(&self) {
        let _ = std::fs::write(self.path.join("cgroup.kill"), "1");
    }

    pub(super) fn remove(&self) -> Result<()> {
        kill_and_remove(&self.path)
    }
}

#[cfg(target_os = "linux")]
pub(super) fn enter(path: &Path) -> Result<()> {
    std::fs::write(path.join("cgroup.procs"), "0")
        .with_context(|| format!("join the cgroup {}", path.display()))
}
