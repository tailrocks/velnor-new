// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use super::super::inventory;

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(super) struct Clock {
    path: PathBuf,
    pub(super) modified: SystemTime,
}

impl Clock {
    pub(super) fn new(target: &Path) -> io::Result<Self> {
        let directory = target.join(inventory::STORE);
        fs::create_dir_all(&directory)?;
        loop {
            let path = directory.join(format!(
                ".build-clock-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    let mut clock = Self {
                        path,
                        modified: SystemTime::UNIX_EPOCH,
                    };
                    clock.modified = file.metadata()?.modified()?;
                    return Ok(clock);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for Clock {
    fn drop(&mut self) {
        let _removed = fs::remove_file(&self.path);
    }
}
