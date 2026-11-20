// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use super::clear::clear_home;
use super::{HOME_PATHS, Inputs, KEEP_KEYS, Outcome, TOOLS_DIR};
use crate::mbx_store::{random_u64, writable_by_job};

const MARKER: &str = ".complete";
const STAGING_PREFIX: &str = ".staging-";
const LOCK_PREFIX: &str = ".lock-";
const STALE_AFTER: Duration = Duration::from_hours(1);
const COPY: &str = "/bin/cp";

fn base(dir: Option<&str>, owner: u32, read_only: bool) -> Result<PathBuf, Outcome> {
    let Some(dir) = dir.filter(|dir| !dir.is_empty()) else {
        return Err(Outcome::NotOffered);
    };
    let dir = Path::new(dir);
    let reject = |reason| Outcome::Rejected(reason);
    if !dir.is_absolute() || dir.components().any(|part| part.as_os_str() == "..") {
        return Err(reject("not an absolute normal path"));
    }
    let metadata = fs::symlink_metadata(dir).map_err(|_| reject("does not exist"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(reject("not a directory"));
    }
    if metadata.uid() != owner && metadata.uid() != 0 {
        return Err(reject("owned by another user"));
    }
    if metadata.mode() & 0o002 != 0 {
        return Err(reject("world-writable"));
    }
    if read_only && writable_by_job(dir) {
        return Err(reject("writable by this job"));
    }
    Ok(dir.to_path_buf())
}

fn complete(entry: &Path, owner: u32) -> bool {
    let sound = |path: &Path| {
        fs::symlink_metadata(path).is_ok_and(|metadata| {
            !metadata.file_type().is_symlink()
                && (metadata.uid() == owner || metadata.uid() == 0)
                && metadata.mode() & 0o002 == 0
        })
    };
    sound(entry)
        && fs::symlink_metadata(entry.join(MARKER)).is_ok_and(|metadata| metadata.is_file())
        && HOME_PATHS.iter().all(|path| sound(&entry.join(slot(path))))
}

fn slot(path: &str) -> String {
    path.replace('/', "_")
}

fn copy_tree(from: &Path, to: &Path) -> bool {
    fs::create_dir_all(to).is_ok()
        && Command::new(COPY)
            .arg("-a")
            .arg(format!("{}/.", from.display()))
            .arg(to)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
}

pub(super) fn restore(inputs: &Inputs<'_>, dir: Option<&str>, read_only: bool) -> Outcome {
    let base = match base(dir, inputs.owner, read_only) {
        Ok(base) => base,
        Err(outcome) => return outcome,
    };
    let entry = base.join(TOOLS_DIR).join(inputs.key);
    if !complete(&entry, inputs.owner) {
        return Outcome::Miss;
    }
    if let Err(reason) = clear_home(inputs.home) {
        return Outcome::Rejected(reason);
    }
    for path in HOME_PATHS {
        if !copy_tree(&entry.join(slot(path)), &inputs.home.join(path)) {
            return Outcome::Rejected("a copy failed, so the cache action restores instead");
        }
    }
    Outcome::Restored { from: entry }
}

struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        fs::remove_dir(&self.0).ok();
    }
}

enum Claim {
    Won(Lock),
    Held,
    Broken,
}

fn stale(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| modified.elapsed().is_ok_and(|age| age > STALE_AFTER))
}

fn claim(tools: &Path, key: &str) -> Claim {
    let path = tools.join(format!("{LOCK_PREFIX}{key}"));
    for _ in 0..2 {
        match fs::create_dir(&path) {
            Ok(()) => return Claim::Won(Lock(path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if !stale(&path) {
                    return Claim::Held;
                }
                fs::remove_dir_all(&path).ok();
            }
            Err(_) => return Claim::Broken,
        }
    }
    Claim::Held
}

fn sweep(tools: &Path) {
    let Ok(entries) = fs::read_dir(tools) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if (name.starts_with(STAGING_PREFIX) || name.starts_with(LOCK_PREFIX))
            && stale(&entry.path())
        {
            fs::remove_dir_all(entry.path()).ok();
        }
    }
}

pub(super) fn save(inputs: &Inputs<'_>) -> Outcome {
    let base = match base(inputs.shared_dir, inputs.owner, false) {
        Ok(base) => base,
        Err(outcome) => return outcome,
    };
    let tools = base.join(TOOLS_DIR);
    let entry = tools.join(inputs.key);
    if complete(&entry, inputs.owner) {
        return Outcome::AlreadySaved;
    }
    if fs::create_dir_all(&tools).is_err() {
        return Outcome::Failed("cannot create the tools directory");
    }
    let _lock = match claim(&tools, inputs.key) {
        Claim::Won(lock) => lock,
        Claim::Held => return Outcome::Busy,
        Claim::Broken => return Outcome::Failed("cannot lock the key"),
    };
    sweep(&tools);
    if complete(&entry, inputs.owner) {
        return Outcome::AlreadySaved;
    }
    let staging = tools.join(format!(
        "{STAGING_PREFIX}{}-{:016x}",
        inputs.key,
        random_u64()
    ));
    let copied = fs::create_dir(&staging).is_ok()
        && HOME_PATHS
            .iter()
            .all(|path| copy_tree(&inputs.home.join(path), &staging.join(slot(path))))
        && fs::File::create(staging.join(MARKER)).is_ok();
    if !copied {
        fs::remove_dir_all(&staging).ok();
        return Outcome::Failed("the copy to the store failed");
    }
    if fs::symlink_metadata(&entry).is_ok() {
        fs::remove_dir_all(&entry).ok();
    }
    if fs::rename(&staging, &entry).is_err() {
        fs::remove_dir_all(&staging).ok();
        return if complete(&entry, inputs.owner) {
            Outcome::AlreadySaved
        } else {
            Outcome::Failed("the entry could not be published")
        };
    }
    prune(&tools, inputs.key);
    Outcome::Saved { to: entry }
}

fn prune(tools: &Path, current: &str) {
    let Ok(entries) = fs::read_dir(tools) else {
        return;
    };
    let mut older: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            name != current && !name.to_string_lossy().starts_with('.')
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    older.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, path) in older.into_iter().skip(KEEP_KEYS - 1) {
        fs::remove_dir_all(path).ok();
    }
}
