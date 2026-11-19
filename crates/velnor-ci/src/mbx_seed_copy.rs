// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

pub const SUBSETS: [&str; 4] = [
    "cas",
    "executable-identities",
    "task-manifests",
    "action-results",
];

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Copied {
    pub files: u64,
    pub bytes: u64,
    pub present: u64,
    pub failed: u64,
    pub expired: bool,
}

struct Job {
    from: PathBuf,
    to: PathBuf,
    size: u64,
}

#[derive(Default)]
struct Walk {
    jobs: Vec<Job>,
    failed: u64,
    expired: bool,
}

enum Copy {
    Wrote,
    Present,
    Failed,
}

fn list(from: &Path, to: &Path, walk: &mut Walk, deadline: Instant) {
    match fs::symlink_metadata(from) {
        Ok(metadata) if metadata.is_dir() => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return,
        _ => {
            walk.failed += 1;
            return;
        }
    }
    if Instant::now() >= deadline {
        walk.expired = true;
        return;
    }
    let Ok(entries) = fs::read_dir(from) else {
        walk.failed += 1;
        return;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            walk.failed += 1;
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            walk.failed += 1;
            continue;
        };
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if kind.is_dir() {
            list(&source, &target, walk, deadline);
        } else if kind.is_file() {
            let size = entry.metadata().map_or(0, |metadata| metadata.len());
            walk.jobs.push(Job {
                from: source,
                to: target,
                size,
            });
        }
    }
}

#[must_use]
pub fn copy_subset(seed: &Path, store: &Path, threads: usize, deadline: Instant) -> Copied {
    let mut total = Copied::default();
    for subset in SUBSETS {
        let mut walk = Walk::default();
        list(&seed.join(subset), &store.join(subset), &mut walk, deadline);
        let part = run(walk.jobs, threads, deadline);
        total.files += part.files;
        total.bytes += part.bytes;
        total.present += part.present;
        total.failed += part.failed + walk.failed;
        total.expired |= part.expired || walk.expired;
    }
    total
}

fn run(jobs: Vec<Job>, threads: usize, deadline: Instant) -> Copied {
    let queue = Mutex::new(jobs.into_iter());
    let files = AtomicU64::new(0);
    let bytes = AtomicU64::new(0);
    let present = AtomicU64::new(0);
    let failed = AtomicU64::new(0);
    let expired = AtomicBool::new(false);
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| {
                loop {
                    if Instant::now() >= deadline {
                        expired.store(true, Ordering::Relaxed);
                        break;
                    }
                    let next = queue.lock().ok().and_then(|mut jobs| jobs.next());
                    let Some(job) = next else { break };
                    match copy_file(&job) {
                        Copy::Wrote => {
                            files.fetch_add(1, Ordering::Relaxed);
                            bytes.fetch_add(job.size, Ordering::Relaxed);
                        }
                        Copy::Present => {
                            present.fetch_add(1, Ordering::Relaxed);
                        }
                        Copy::Failed => {
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            });
        }
    });
    Copied {
        files: files.into_inner(),
        bytes: bytes.into_inner(),
        present: present.into_inner(),
        failed: failed.into_inner(),
        expired: expired.into_inner(),
    }
}

fn copy_file(job: &Job) -> Copy {
    if fs::symlink_metadata(&job.to).is_ok() {
        return Copy::Present;
    }
    transfer(job).unwrap_or(Copy::Failed)
}

fn transfer(job: &Job) -> Option<Copy> {
    let parent = job.to.parent()?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .ok()?;
    let mut source = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&job.from)
        .ok()?;
    if !source.metadata().ok()?.is_file() {
        return None;
    }
    let name = job.to.file_name()?.to_string_lossy().into_owned();
    let partial = parent.join(format!(".{name}.seed{}", std::process::id()));
    let mut target = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&partial)
        .ok()?;
    let done = std::io::copy(&mut source, &mut target)
        .and_then(|_| target.flush())
        .and_then(|()| fs::rename(&partial, &job.to));
    match done {
        Ok(()) => Some(Copy::Wrote),
        Err(error) => {
            fs::remove_file(&partial).ok();
            Some(if error.kind() == ErrorKind::AlreadyExists {
                Copy::Present
            } else {
                Copy::Failed
            })
        }
    }
}
