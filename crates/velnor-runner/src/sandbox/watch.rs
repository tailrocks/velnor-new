// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use super::cgroup::Cgroup;
use super::{Layout, PIDS_MAX, disk, floor};

pub(super) const WATCH_EVERY: Duration = Duration::from_millis(100);
const WALK_EVERY: Duration = Duration::from_secs(1);
const WALK_BUDGET: Duration = Duration::from_secs(1);
const GIB: u64 = 1 << 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Breach {
    Disk(disk::Usage),
    ForeignDevice,
    MachineFull,
    Processes,
    Memory,
    Killed,
}

impl Breach {
    pub(super) const fn kind(self) -> &'static str {
        match self {
            Self::Disk(_) => "disk",
            Self::ForeignDevice => "foreign_device",
            Self::MachineFull => "machine_full",
            Self::Processes => "processes",
            Self::Memory => "memory",
            Self::Killed => "killed",
        }
    }

    pub(super) fn message(self, limit_gib: u64) -> String {
        match self {
            Self::Disk(usage) => disk::overage(usage, limit_gib).unwrap_or_default(),
            Self::ForeignDevice => {
                "velnor: job created a mount or a subvolume under its directories".to_owned()
            }
            Self::MachineFull => "velnor: the machine's disk is nearly full".to_owned(),
            Self::Processes => format!("velnor: job started more than {PIDS_MAX} processes"),
            Self::Memory => {
                "velnor: job was killed for using more memory than its limit".to_owned()
            }
            Self::Killed => "velnor: the job killed the process the sandbox waits for".to_owned(),
        }
    }
}

pub(super) struct Limits {
    pub(super) disk_gib: u64,
    pub(super) floor_gib: u64,
}

pub(super) struct Walk {
    started: Instant,
    pub(super) stop: Arc<AtomicBool>,
    result: Receiver<disk::Usage>,
}

impl Drop for Walk {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub(super) struct Watch {
    walk: Option<Walk>,
    next: Instant,
}

fn machine_full(floor_gib: u64) -> bool {
    floor_gib > 0 && floor::free_bytes().is_none_or(|free| free < floor_gib.saturating_mul(GIB))
}

pub(super) fn start(layout: &Layout) -> Option<Walk> {
    let directories: Vec<PathBuf> = vec![layout.work_dir.clone(), layout.stage_dir.clone()];
    let stop = Arc::new(AtomicBool::new(false));
    let walking = Arc::clone(&stop);
    let (sender, result) = channel();
    std::thread::Builder::new()
        .name("disk-watch".to_owned())
        .spawn(move || {
            let directories: Vec<&Path> = directories.iter().map(PathBuf::as_path).collect();
            let _ = sender.send(disk::used(&directories, &walking));
        })
        .ok()?;
    Some(Walk {
        started: Instant::now(),
        stop,
        result,
    })
}

fn verdict(usage: disk::Usage, limits: &Limits) -> Option<Breach> {
    if usage.foreign {
        return Some(Breach::ForeignDevice);
    }
    disk::overage(usage, limits.disk_gib).map(|_| Breach::Disk(usage))
}

impl Watch {
    pub(super) fn settle(
        &mut self,
        layout: &Layout,
        cgroup: &Cgroup,
        limits: &Limits,
    ) -> Option<Breach> {
        if cgroup.exhausted_processes() {
            return Some(Breach::Processes);
        }
        if machine_full(limits.floor_gib) {
            return Some(Breach::MachineFull);
        }
        let walk = self.walk.take().or_else(|| start(layout))?;
        let left = WALK_BUDGET.saturating_sub(walk.started.elapsed());
        if let Ok(usage) = walk.result.recv_timeout(left) {
            return verdict(usage, limits);
        }
        walk.stop.store(true, Ordering::Relaxed);
        None
    }

    pub(super) fn new() -> Self {
        Self {
            walk: None,
            next: Instant::now() + WALK_EVERY,
        }
    }

    pub(super) fn poll(
        &mut self,
        layout: &Layout,
        cgroup: &Cgroup,
        limits: &Limits,
    ) -> Option<Breach> {
        if cgroup.exhausted_processes() {
            return Some(Breach::Processes);
        }
        if machine_full(limits.floor_gib) {
            return Some(Breach::MachineFull);
        }
        let Some(walk) = &self.walk else {
            if Instant::now() >= self.next {
                self.walk = start(layout);
            }
            return None;
        };
        match walk.result.try_recv() {
            Ok(usage) => {
                self.next = walk.started + WALK_EVERY;
                self.walk = None;
                verdict(usage, limits)
            }
            Err(TryRecvError::Empty) => {
                if walk.started.elapsed() > WALK_BUDGET {
                    walk.stop.store(true, Ordering::Relaxed);
                }
                None
            }
            Err(TryRecvError::Disconnected) => {
                self.walk = None;
                None
            }
        }
    }
}
