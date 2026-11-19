// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);
const MAX_TEMP_DIRECTORY_ATTEMPTS: usize = 1_024;

pub(crate) struct TestTempDir(PathBuf);

impl TestTempDir {
    pub(crate) fn new(prefix: &str) -> io::Result<Self> {
        unique_temp_dir(prefix).map(Self)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestTempDir {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!("velnor_temp_test_cleanup_failed:{:?}", error.kind());
        }
    }
}

pub(crate) fn unique_temp_dir(prefix: &str) -> io::Result<PathBuf> {
    unique_temp_dir_in(&std::env::temp_dir(), prefix)
}

pub(crate) fn unique_temp_dir_in(parent: &Path, prefix: &str) -> io::Result<PathBuf> {
    unique_temp_dir_in_with_counter(parent, prefix, &NEXT_TEMP_DIRECTORY_ID)
}

fn unique_temp_dir_in_with_counter(
    parent: &Path,
    prefix: &str,
    counter: &AtomicU64,
) -> io::Result<PathBuf> {
    let process_id = std::process::id();
    for _ in 0..MAX_TEMP_DIRECTORY_ATTEMPTS {
        let id = reserve_temp_directory_id(counter)?;
        let candidate = parent.join(format!("{prefix}-{process_id}-{id}"));
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "temporary directory namespace exhausted after 1024 occupied names",
    ))
}

fn reserve_temp_directory_id(counter: &AtomicU64) -> io::Result<u64> {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let next = current
            .checked_add(1)
            .ok_or_else(|| io::Error::other("temporary directory ID space exhausted"))?;
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return Ok(current),
            Err(observed) => current = observed,
        }
    }
}




