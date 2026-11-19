// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

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
}

struct Job {
    from: PathBuf,
    to: PathBuf,
    size: u64,
}

fn list(from: &Path, to: &Path, jobs: &mut Vec<Job>) {
    let Ok(entries) = fs::read_dir(from) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if kind.is_dir() {
            list(&source, &target, jobs);
        } else if kind.is_file() {
            let size = entry.metadata().map_or(0, |metadata| metadata.len());
            jobs.push(Job {
                from: source,
                to: target,
                size,
            });
        }
    }
}

#[must_use]
pub fn copy_subset(seed: &Path, store: &Path, threads: usize) -> Copied {
    let mut total = Copied::default();
    for subset in SUBSETS {
        let mut jobs = Vec::new();
        list(&seed.join(subset), &store.join(subset), &mut jobs);
        let part = run(jobs, threads);
        total.files += part.files;
        total.bytes += part.bytes;
        total.present += part.present;
    }
    total
}

fn run(jobs: Vec<Job>, threads: usize) -> Copied {
    let queue = Mutex::new(jobs.into_iter());
    let files = AtomicU64::new(0);
    let bytes = AtomicU64::new(0);
    let present = AtomicU64::new(0);
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().ok().and_then(|mut jobs| jobs.next());
                    let Some(job) = next else { break };
                    match copy_file(&job) {
                        Some(true) => {
                            files.fetch_add(1, Ordering::Relaxed);
                            bytes.fetch_add(job.size, Ordering::Relaxed);
                        }
                        Some(false) => {
                            present.fetch_add(1, Ordering::Relaxed);
                        }
                        None => {}
                    }
                }
            });
        }
    });
    Copied {
        files: files.into_inner(),
        bytes: bytes.into_inner(),
        present: present.into_inner(),
    }
}

fn copy_file(job: &Job) -> Option<bool> {
    if fs::symlink_metadata(&job.to).is_ok() {
        return Some(false);
    }
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
        Ok(()) => Some(true),
        Err(error) => {
            fs::remove_file(&partial).ok();
            (error.kind() == ErrorKind::AlreadyExists).then_some(false)
        }
    }
}
