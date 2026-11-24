// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use super::inputs::{publish_snapshot, set_modified_ns};
use crate::error::CiError;
use crate::input_dirs::{
    Entry, EntryKind, Listings, candidates, decode, directories, encode, plan_times,
};
use crate::inputs::SavedInput;

fn list(path: &Path) -> Option<Vec<Entry>> {
    fs::read_dir(path)
        .ok()?
        .map(|entry| {
            let entry = entry.ok()?;
            let kind = entry.file_type().ok()?;
            Some(Entry {
                name: entry.file_name(),
                kind: if kind.is_file() {
                    EntryKind::File
                } else if kind.is_dir() {
                    EntryKind::Directory
                } else {
                    EntryKind::Other
                },
            })
        })
        .collect()
}

pub(super) fn save(
    top: &Path,
    path: &Path,
    inputs_text: &str,
    saved: &[SavedInput],
) -> Result<(), CiError> {
    let listings: Listings = directories(saved)
        .into_iter()
        .filter_map(|directory| list(&top.join(&directory)).map(|entries| (directory, entries)))
        .collect();
    publish_snapshot(path, &encode(inputs_text, &listings))
}

pub(super) fn restore(
    top: &Path,
    path: &Path,
    inputs_text: &str,
    restored: &[SavedInput],
) -> Result<usize, CiError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(CiError::new(format!(
                "cannot read {}: {error}",
                path.display()
            )));
        }
    };
    let recorded = decode(&text, inputs_text);
    let mut live = Listings::new();
    for directory in candidates(&recorded, restored) {
        let absolute = top.join(&directory);
        if directory
            .rsplit_once('/')
            .is_none_or(|(parent, _)| live.contains_key(parent))
            && fs::symlink_metadata(&absolute).is_ok_and(|metadata| metadata.is_dir())
            && let Some(entries) = list(&absolute)
        {
            live.insert(directory, entries);
        }
    }
    let times = plan_times(&recorded, &live, restored);
    for (directory, mtime_ns) in &times {
        set_modified_ns(&top.join(directory), *mtime_ns)?;
    }
    Ok(times.len())
}
