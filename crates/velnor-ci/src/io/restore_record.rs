use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime};

use crate::error::CiError;

const RECORD: &str = "velnor-restore-record";
const TICK: Duration = Duration::from_millis(20);

type Change = (i64, i64);
type Inode = (u64, u64);

struct Entry {
    path: PathBuf,
    inode: Inode,
    change: Change,
}

#[derive(Default)]
pub(in crate::io) struct Record {
    vouched: HashMap<Inode, Change>,
}

impl Record {
    pub(in crate::io) fn read(root: &Path) -> Self {
        Self {
            vouched: entries(root)
                .into_iter()
                .map(|entry| (entry.inode, entry.change))
                .collect(),
        }
    }

    pub(in crate::io) fn vouches(&self, inode: Inode, change: Change) -> bool {
        self.vouched.get(&inode) == Some(&change)
    }
}

fn entries(root: &Path) -> Vec<Entry> {
    git_path(root, RECORD)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .unwrap_or_default()
        .lines()
        .filter_map(parse)
        .collect()
}

fn parse(line: &str) -> Option<Entry> {
    let mut fields = line.splitn(5, '\t');
    let seconds = fields.next()?.parse().ok()?;
    let nanos = fields.next()?.parse().ok()?;
    let device = fields.next()?.parse().ok()?;
    let inode = fields.next()?.parse().ok()?;
    Some(Entry {
        path: PathBuf::from(fields.next()?),
        inode: (device, inode),
        change: (seconds, nanos),
    })
}

fn observe(path: &Path) -> Option<Entry> {
    let metadata = fs::symlink_metadata(path).ok()?;
    Some(Entry {
        path: path.to_path_buf(),
        inode: (metadata.dev(), metadata.ino()),
        change: (metadata.ctime(), metadata.ctime_nsec()),
    })
}

pub(super) fn git_path(root: &Path, name: &str) -> Result<PathBuf, CiError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--path-format=absolute", "--git-path", name])
        .output()
        .map_err(|e| failed("locate", root, e))?;
    if !output.status.success() {
        return Err(failed(
            "locate",
            root,
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    let path = String::from_utf8(output.stdout).map_err(|e| failed("locate", root, e))?;
    Ok(PathBuf::from(path.trim_end_matches(['\n', '\r'])))
}

#[derive(Default)]
pub(super) struct Restored {
    pub(super) top: Option<PathBuf>,
    pub(super) scopes: Vec<PathBuf>,
    pub(super) paths: Vec<PathBuf>,
}

pub(super) fn record(root: &Path, restored: &Restored) -> Result<(), CiError> {
    let path = git_path(root, RECORD)?;
    let scopes: Vec<PathBuf> = restored
        .scopes
        .iter()
        .filter_map(|scope| fs::canonicalize(scope).ok())
        .collect();
    let kept: Vec<Entry> = entries(root)
        .into_iter()
        .filter(|old| !scopes.iter().any(|scope| old.path.starts_with(scope)))
        .filter(|old| {
            observe(&old.path).is_some_and(|now| (now.inode, now.change) == (old.inode, old.change))
        })
        .collect();
    let ceiling = restored
        .top
        .as_ref()
        .and_then(|top| fs::canonicalize(top).ok());
    let mut parents: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
    let mut paths = BTreeSet::new();
    for file in &restored.paths {
        let (Some(parent), Some(name)) = (file.parent(), file.file_name()) else {
            continue;
        };
        let Some(parent) = parents
            .entry(parent.to_path_buf())
            .or_insert_with(|| fs::canonicalize(parent).ok())
            .clone()
        else {
            continue;
        };
        let file = parent.join(name);
        if let Some(ceiling) = &ceiling {
            for directory in file
                .ancestors()
                .skip(1)
                .take_while(|directory| directory.starts_with(ceiling))
            {
                paths.insert(directory.to_path_buf());
            }
        }
        paths.insert(file);
    }
    let fresh: Vec<Entry> = paths.iter().filter_map(|path| observe(path)).collect();
    let text: String = kept
        .iter()
        .chain(&fresh)
        .filter_map(|entry| {
            let name = entry.path.to_str().filter(|name| !name.contains('\n'))?;
            let ((seconds, nanos), (device, inode)) = (entry.change, entry.inode);
            Some(format!("{seconds}\t{nanos}\t{device}\t{inode}\t{name}\n"))
        })
        .collect();
    fs::write(&path, text).map_err(|e| failed("write", &path, e))?;
    settle(&fresh);
    Ok(())
}

fn settle(fresh: &[Entry]) {
    let Some(newest) = fresh.iter().map(|entry| entry.change).max() else {
        return;
    };
    let coarse = fresh.iter().all(|entry| entry.change.1 == 0);
    thread::sleep(pause(newest, coarse, SystemTime::now()));
}

fn pause(newest: Change, coarse: bool, now: SystemTime) -> Duration {
    let seconds = u64::try_from(newest.0).unwrap_or(0);
    let nanos = u32::try_from(newest.1).unwrap_or(0);
    let clear = if coarse {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds.saturating_add(1))
    } else {
        SystemTime::UNIX_EPOCH + Duration::new(seconds, nanos) + TICK
    };
    clear.duration_since(now).unwrap_or_default()
}

fn failed(what: &str, path: &Path, error: impl std::fmt::Display) -> CiError {
    CiError::new(format!(
        "cannot {what} the restore record {}: {error}",
        path.display()
    ))
}

#[cfg(test)]
mod tests;
