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
    TooManyFiles,
    ForeignDevice,
    MachineFull,
    Processes,
}

impl Breach {
    pub(super) fn message(self, limit_gib: u64) -> String {
        match self {
            Self::Disk(usage) => disk::overage(usage, limit_gib).unwrap_or_default(),
            Self::TooManyFiles => {
                "velnor: job has too many files to account for its disk use".to_owned()
            }
            Self::ForeignDevice => {
                "velnor: job created a mount or a subvolume under its directories".to_owned()
            }
            Self::MachineFull => "velnor: the machine's disk is nearly full".to_owned(),
            Self::Processes => format!("velnor: job started more than {PIDS_MAX} processes"),
        }
    }
}

pub(super) struct Limits {
    pub(super) disk_gib: u64,
    pub(super) floor_gib: u64,
}

struct Walk {
    started: Instant,
    stop: Arc<AtomicBool>,
    result: Receiver<disk::Usage>,
}

pub(super) struct Watch {
    walk: Option<Walk>,
    next: Instant,
}

fn machine_full(floor_gib: u64) -> bool {
    floor_gib > 0 && floor::free_bytes().is_none_or(|free| free < floor_gib.saturating_mul(GIB))
}

fn start(layout: &Layout, cgroup: &Cgroup) -> Option<Walk> {
    let directories: Vec<PathBuf> = vec![layout.work_dir.clone(), layout.stage_dir.clone()];
    let held = cgroup.path().to_path_buf();
    let stop = Arc::new(AtomicBool::new(false));
    let walking = Arc::clone(&stop);
    let (sender, result) = channel();
    std::thread::Builder::new()
        .name("disk-watch".to_owned())
        .spawn(move || {
            let directories: Vec<&Path> = directories.iter().map(PathBuf::as_path).collect();
            let _ = sender.send(disk::used(&directories, &held, &walking));
        })
        .ok()?;
    Some(Walk {
        started: Instant::now(),
        stop,
        result,
    })
}

impl Watch {
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
                match start(layout, cgroup) {
                    Some(walk) => self.walk = Some(walk),
                    None => return Some(Breach::TooManyFiles),
                }
            }
            return None;
        };
        match walk.result.try_recv() {
            Ok(usage) => {
                self.next = walk.started + WALK_EVERY;
                self.walk = None;
                if usage.foreign {
                    return Some(Breach::ForeignDevice);
                }
                disk::overage(usage, limits.disk_gib).map(|_| Breach::Disk(usage))
            }
            Err(TryRecvError::Empty) if walk.started.elapsed() <= WALK_BUDGET => None,
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                walk.stop.store(true, Ordering::Relaxed);
                Some(Breach::TooManyFiles)
            }
        }
    }
}
