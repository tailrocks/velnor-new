// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, TryLockError};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::clear::{check_home, clear_home};
use super::lease::{Identity, identity, open_entry, published};
use super::{HOME_PATHS, Inputs, KEEP_KEYS, Outcome, TOOLS_DIR, valid_key};
use crate::mbx_store::{random_u64, writable_by_job};
use crate::tree_removal::remove_tree;

const MARKER: &str = ".complete";
const STAGING_PREFIX: &str = ".staging-";
const RETIRED_PREFIX: &str = "retired-";
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

fn lease(entry: &Path, owner: u32) -> Option<(File, bool)> {
    let held = open_entry(entry).ok()?;
    let leased = held.try_lock_shared().is_ok();
    (published(&held, entry) && complete(entry, owner)).then_some((held, leased))
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

pub(super) fn restore(
    inputs: &Inputs<'_>,
    dir: Option<&str>,
    read_only: bool,
    owner: u32,
) -> Outcome {
    restore_with(inputs, dir, read_only, owner, &|| {})
}

fn restore_with(
    inputs: &Inputs<'_>,
    dir: Option<&str>,
    read_only: bool,
    owner: u32,
    copied: &dyn Fn(),
) -> Outcome {
    if let Err(reason) = check_home(inputs.home) {
        return Outcome::Rejected(reason);
    }
    let base = match base(dir, owner, read_only) {
        Ok(base) => base,
        Err(outcome) => return outcome,
    };
    let entry = base.join(TOOLS_DIR).join(inputs.key);
    let Some((held, leased)) = lease(&entry, owner) else {
        return Outcome::Miss;
    };
    if let Err(reason) = clear_home(inputs.home) {
        return Outcome::Rejected(reason);
    }
    for path in HOME_PATHS {
        if !copy_tree(&entry.join(slot(path)), &inputs.home.join(path)) {
            return Outcome::Rejected("a copy failed, so the cache action restores instead");
        }
        copied();
    }
    if !published(&held, &entry) {
        return Outcome::Rejected(
            "the entry was replaced or removed during the copy, so the cache action restores instead",
        );
    }
    Outcome::Restored {
        from: entry,
        leased,
    }
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

fn owners(name: &str) -> Vec<&str> {
    let rest = name.strip_prefix(STAGING_PREFIX).unwrap_or(name);
    let mut keys = Vec::new();
    if let Some(key) = rest.strip_prefix(RETIRED_PREFIX) {
        keys.push(key);
    }
    if let Some((key, suffix)) = rest.rsplit_once('-')
        && suffix.len() == 16
        && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        keys.push(key);
    }
    keys
}

fn hold_owners(tools: &Path, name: &str, current: &str) -> Option<Vec<Lock>> {
    let mut held = Vec::new();
    for key in owners(name).into_iter().filter(|key| *key != current) {
        match claim(tools, key) {
            Claim::Won(lock) => held.push(lock),
            Claim::Held | Claim::Broken => return None,
        }
    }
    Some(held)
}

fn sweep(tools: &Path, current: &str) {
    let Ok(entries) = fs::read_dir(tools) else {
        return;
    };
    let entries: Vec<_> = entries.flatten().collect();
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let leftover = name.starts_with(STAGING_PREFIX);
        if !(leftover || name.starts_with(LOCK_PREFIX))
            || entry.file_type().is_ok_and(|kind| kind.is_file())
            || !stale(&entry.path())
        {
            continue;
        }
        let _held = if leftover {
            match hold_owners(tools, &name, current) {
                Some(held) => held,
                None => continue,
            }
        } else {
            Vec::new()
        };
        remove_tree(&entry.path()).ok();
    }
}

pub(super) fn save(inputs: &Inputs<'_>, owner: u32) -> Outcome {
    save_with(inputs, owner, &|| {})
}

fn save_with(inputs: &Inputs<'_>, owner: u32, chosen: &dyn Fn()) -> Outcome {
    let base = match base(inputs.shared_dir, owner, false) {
        Ok(base) => base,
        Err(outcome) => return outcome,
    };
    let tools = base.join(TOOLS_DIR);
    let entry = tools.join(inputs.key);
    if !inputs.replace && complete(&entry, owner) {
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
    sweep(&tools, inputs.key);
    if !inputs.replace && complete(&entry, owner) {
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
        remove_tree(&staging).ok();
        return Outcome::Failed("the copy to the store failed");
    }
    let retired = tools.join(format!("{STAGING_PREFIX}{RETIRED_PREFIX}{}", inputs.key));
    if fs::remove_dir_all(&retired).is_err()
        && fs::symlink_metadata(&retired).is_ok_and(|metadata| metadata.is_dir())
    {
        remove_tree(&retired).ok();
    }
    let present = fs::symlink_metadata(&entry).is_ok();
    let replaced = present && fs::rename(&entry, &retired).is_ok();
    if inputs.replace && present && !replaced {
        remove_tree(&staging).ok();
        return Outcome::Failed("the poisoned entry could not be retired");
    }
    if fs::rename(&staging, &entry).is_err() {
        if replaced {
            fs::rename(&retired, &entry).ok();
        }
        remove_tree(&staging).ok();
        return if !inputs.replace && complete(&entry, owner) {
            Outcome::AlreadySaved
        } else {
            Outcome::Failed("the entry could not be published")
        };
    }
    if replaced && remove_tree(&retired).is_err() {
        return Outcome::Failed("the replaced entry could not be removed");
    }
    let kept = prune(&tools, inputs.key, chosen);
    if replaced {
        Outcome::Healed { to: entry, kept }
    } else {
        Outcome::Saved { to: entry, kept }
    }
}

fn prune(tools: &Path, current: &str, chosen: &dyn Fn()) -> Vec<String> {
    let Ok(entries) = fs::read_dir(tools) else {
        return Vec::new();
    };
    let mut older: Vec<(Identity, PathBuf)> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            name != current && !name.to_string_lossy().starts_with('.')
        })
        .filter_map(|entry| Some((identity(&entry.metadata().ok()?), entry.path())))
        .collect();
    older.sort_by_key(|(listed, _)| std::cmp::Reverse((listed.2, listed.3)));
    let candidates: Vec<_> = older.into_iter().skip(KEEP_KEYS - 1).collect();
    chosen();
    let mut in_use = 0;
    candidates
        .into_iter()
        .filter(
            |(listed, path)| match retire(tools, path, *listed, in_use < KEEP_KEYS) {
                Retired::Settled => false,
                Retired::InUse => {
                    in_use += 1;
                    false
                }
                Retired::Stuck => true,
            },
        )
        .map(|(_, path)| plain(&path.file_name().unwrap_or_default().to_string_lossy()))
        .collect()
}

enum Retired {
    Settled,
    InUse,
    Stuck,
}

fn retire(tools: &Path, path: &Path, listed: Identity, spare_in_use: bool) -> Retired {
    let finished = |removal: std::io::Result<()>| match removal {
        Ok(()) => Retired::Settled,
        Err(_) => Retired::Stuck,
    };
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if !valid_key(&name) {
        return finished(remove_tree(path));
    }
    let _lock = match claim(tools, &name) {
        Claim::Won(lock) => lock,
        Claim::Held => return Retired::Settled,
        Claim::Broken => return Retired::Stuck,
    };
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Retired::Settled;
    };
    if identity(&metadata) != listed {
        return Retired::Settled;
    }
    let _held = if metadata.is_dir() {
        let Ok(held) = open_entry(path) else {
            return Retired::Stuck;
        };
        match held.try_lock() {
            Ok(()) => Some(held),
            Err(TryLockError::WouldBlock) if spare_in_use => return Retired::InUse,
            Err(TryLockError::WouldBlock) => None,
            Err(TryLockError::Error(_)) => return Retired::Stuck,
        }
    } else {
        None
    };
    let grave = tools.join(format!("{STAGING_PREFIX}{name}-{:016x}", random_u64()));
    finished(fs::rename(path, &grave).and_then(|()| remove_tree(&grave)))
}

fn plain(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
