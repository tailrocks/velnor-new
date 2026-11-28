// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use rustix::process::{Pid, Signal, kill_process};

use super::{MEMORY_PERCENT, PIDS_MAX};

const POLL: Duration = Duration::from_millis(5);
const EMPTY_WITHIN: Duration = Duration::from_secs(10);
const CONTROLLERS: &str = "+pids +memory";

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

pub(super) fn kill_and_remove(path: &Path) -> Result<()> {
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

pub(super) fn limit_hit(events: &str) -> bool {
    events
        .lines()
        .filter_map(|line| line.strip_prefix("max "))
        .any(|count| count.trim().parse::<u64>().is_ok_and(|count| count > 0))
}

pub(super) fn memory_limit(total_kib: u64, max_jobs: u32) -> u64 {
    (total_kib.saturating_mul(1024) / 100 * MEMORY_PERCENT) / u64::from(max_jobs.max(1))
}

fn memory_max_bytes(max_jobs: u32) -> Result<u64> {
    let meminfo =
        std::fs::read_to_string("/proc/meminfo").context("read the memory of the machine")?;
    let kib = meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .context("find MemTotal in /proc/meminfo")?;
    Ok(memory_limit(kib, max_jobs))
}

fn write(path: &Path, file: &str, value: &str) -> Result<()> {
    std::fs::write(path.join(file), value)
        .with_context(|| format!("write {value} to {}", path.join(file).display()))
}

impl Cgroup {
    pub(super) fn create(path: PathBuf, max_jobs: u32) -> Result<Self> {
        kill_and_remove(&path)?;
        let parent = path.parent().context("the cgroup has no parent")?;
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        write(parent, "cgroup.subtree_control", CONTROLLERS)?;
        std::fs::create_dir(&path).with_context(|| format!("create {}", path.display()))?;
        let cgroup = Self { path };
        cgroup.limit(max_jobs)?;
        Ok(cgroup)
    }

    fn limit(&self, max_jobs: u32) -> Result<()> {
        write(&self.path, "pids.max", &PIDS_MAX.to_string())?;
        write(
            &self.path,
            "memory.max",
            &memory_max_bytes(max_jobs)?.to_string(),
        )?;
        write(&self.path, "memory.oom.group", "1")
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

    pub(super) fn exhausted_processes(&self) -> bool {
        std::fs::read_to_string(self.path.join("pids.events"))
            .is_ok_and(|events| limit_hit(&events))
    }
}

#[cfg(target_os = "linux")]
pub(super) fn enter(path: &Path) -> Result<()> {
    write(path, "cgroup.procs", "0")
}
