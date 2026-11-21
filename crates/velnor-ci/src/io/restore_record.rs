// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::CiError;

const RECORD: &str = "velnor-restore-record";

type Change = (i64, i64);

#[derive(Default)]
pub(in crate::io) struct Record {
    vouched: HashMap<PathBuf, Change>,
}

impl Record {
    pub(in crate::io) fn read(root: &Path) -> Self {
        let text = git_path(root, RECORD)
            .ok()
            .and_then(|path| fs::read_to_string(path).ok())
            .unwrap_or_default();
        Self {
            vouched: text.lines().filter_map(parse).collect(),
        }
    }

    pub(in crate::io) fn vouches(&self, path: &Path, change: Change) -> bool {
        self.vouched.get(path) == Some(&change)
    }

    pub(in crate::io) fn vouches_entry(&self, path: &Path, change: Change) -> bool {
        entry(path).is_some_and(|path| self.vouches(&path, change))
    }
}

fn parse(line: &str) -> Option<(PathBuf, Change)> {
    let mut fields = line.splitn(3, '\t');
    let seconds = fields.next()?.parse().ok()?;
    let nanos = fields.next()?.parse().ok()?;
    Some((PathBuf::from(fields.next()?), (seconds, nanos)))
}

fn entry(path: &Path) -> Option<PathBuf> {
    Some(
        fs::canonicalize(path.parent()?)
            .ok()?
            .join(path.file_name()?),
    )
}

fn change_of(path: &Path) -> Option<Change> {
    let metadata = fs::symlink_metadata(path).ok()?;
    Some((metadata.ctime(), metadata.ctime_nsec()))
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

pub(super) fn clear(root: &Path) -> Result<(), CiError> {
    remove(&git_path(root, RECORD)?)
}

#[derive(Default)]
pub(super) struct Restored {
    pub(super) top: Option<PathBuf>,
    pub(super) paths: Vec<PathBuf>,
}

pub(super) fn record(root: &Path, restored: &Restored) -> Result<(), CiError> {
    let path = git_path(root, RECORD)?;
    let mut vouched: HashMap<PathBuf, Change> = Record::read(root)
        .vouched
        .into_iter()
        .filter(|(path, change)| change_of(path) == Some(*change))
        .collect();
    let ceiling = restored
        .top
        .as_ref()
        .and_then(|top| fs::canonicalize(top).ok());
    let mut parents: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
    let mut entries = BTreeSet::new();
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
                entries.insert(directory.to_path_buf());
            }
        }
        entries.insert(file);
    }
    for entry in entries {
        if let Some(change) = change_of(&entry) {
            vouched.insert(entry, change);
        }
    }
    let text: String = vouched
        .iter()
        .filter_map(|(entry, (seconds, nanos))| {
            let entry = entry.to_str().filter(|entry| !entry.contains('\n'))?;
            Some(format!("{seconds}\t{nanos}\t{entry}\n"))
        })
        .collect();
    fs::write(&path, text).map_err(|e| failed("write", &path, e))
}

fn remove(path: &Path) -> Result<(), CiError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(failed("remove", path, e)),
    }
}

fn failed(what: &str, path: &Path, error: impl std::fmt::Display) -> CiError {
    CiError::new(format!(
        "cannot {what} the restore record {}: {error}",
        path.display()
    ))
}

#[cfg(test)]
mod tests;
