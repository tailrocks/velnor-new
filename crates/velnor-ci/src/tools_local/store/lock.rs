// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use super::{LOCK_PREFIX, Step, stale};

pub(super) struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        fs::remove_dir(&self.0).ok();
    }
}

pub(super) enum Claim {
    Won(Lock),
    Held,
    Broken,
}

pub(super) fn claim(tools: &Path, key: &str, pause: &dyn Fn(Step)) -> Claim {
    let path = tools.join(format!("{LOCK_PREFIX}{key}"));
    for _ in 0..2 {
        match fs::create_dir(&path) {
            Ok(()) => return Claim::Won(Lock(path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if !stale(&path) {
                    return Claim::Held;
                }
                pause(Step::Breaking);
                fs::remove_dir_all(&path).ok();
            }
            Err(_) => return Claim::Broken,
        }
    }
    Claim::Held
}
