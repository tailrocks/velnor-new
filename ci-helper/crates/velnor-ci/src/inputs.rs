// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::error::CiError;

pub const INPUTS_FILE: &str = "target/.velnor/inputs.tsv";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TrackedFile {
    pub path: String,
    pub blob: String,
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedInput {
    pub file: TrackedFile,
    pub mtime_ns: u64,
}

pub const SYMLINK_MODE: &str = "120000";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tracked {
    pub files: Vec<TrackedFile>,
    pub links: Vec<TrackedFile>,
    pub absent: Vec<String>,
    pub unsupported: usize,
    pub unverifiable_links: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestorePlan {
    pub restore: Vec<SavedInput>,
    pub changed: usize,
    pub new: usize,
    pub deleted: usize,
    pub absent: usize,
    pub unsupported: usize,
    pub links_changed: usize,
}

enum Entry {
    File(TrackedFile),
    Link(TrackedFile),
    Unsupported,
    UnverifiableLink,
    Skipped,
}

#[must_use]
pub fn parse_ls_files(staged: &[u8]) -> Tracked {
    let mut tracked = Tracked::default();
    for record in staged.split(|byte| *byte == 0).filter(|r| !r.is_empty()) {
        match parse_ls_files_record(record) {
            Entry::File(file) => tracked.files.push(file),
            Entry::Link(link) => tracked.links.push(link),
            Entry::Unsupported => tracked.unsupported += 1,
            Entry::UnverifiableLink => tracked.unverifiable_links += 1,
            Entry::Skipped => {}
        }
    }
    tracked
}

#[must_use]
pub fn is_representable(text: &str) -> bool {
    !text.is_empty() && !text.contains(['\t', '\n', '\r'])
}

fn parse_ls_files_record(record: &[u8]) -> Entry {
    let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
        return Entry::Skipped;
    };
    let Ok(meta) = std::str::from_utf8(&record[..tab]) else {
        return Entry::Skipped;
    };
    let mut fields = meta.split(' ');
    let (Some(mode), Some(blob), Some("0")) = (fields.next(), fields.next(), fields.next()) else {
        return Entry::Skipped;
    };
    let link = mode == SYMLINK_MODE;
    if !link && mode != "100644" && mode != "100755" {
        return Entry::Skipped;
    }
    match std::str::from_utf8(&record[tab + 1..]) {
        Ok(path) if is_representable(path) => {
            let entry = TrackedFile {
                path: path.to_owned(),
                blob: blob.to_owned(),
                mode: mode.to_owned(),
            };
            if link {
                Entry::Link(entry)
            } else {
                Entry::File(entry)
            }
        }
        _ if link => Entry::UnverifiableLink,
        _ => Entry::Unsupported,
    }
}

pub fn with_worktree_blobs(
    files: Vec<TrackedFile>,
    hashes: &str,
) -> Result<Vec<TrackedFile>, CiError> {
    let hashes: Vec<&str> = hashes.lines().collect();
    if hashes.len() != files.len() {
        return Err(CiError::new(format!(
            "git hash-object returned {} ids for {} files",
            hashes.len(),
            files.len()
        )));
    }
    Ok(files
        .into_iter()
        .zip(hashes)
        .map(|(file, blob)| TrackedFile {
            blob: blob.to_owned(),
            ..file
        })
        .collect())
}

#[must_use]
pub fn format_inputs(inputs: &[SavedInput]) -> String {
    let mut text = String::new();
    for input in inputs {
        for field in [
            input.file.path.as_str(),
            input.file.blob.as_str(),
            &input.mtime_ns.to_string(),
        ] {
            text.push_str(field);
            text.push('\t');
        }
        text.push_str(&input.file.mode);
        text.push('\n');
    }
    text
}

pub fn parse_inputs(text: &str) -> Result<Vec<SavedInput>, CiError> {
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            parse_inputs_line(line).ok_or_else(|| {
                CiError::new(format!(
                    "{INPUTS_FILE} line {} is not `path<TAB>blob<TAB>mtime_ns<TAB>mode`: `{line}`",
                    index + 1
                ))
            })
        })
        .collect()
}

fn parse_inputs_line(line: &str) -> Option<SavedInput> {
    let mut fields = line.split('\t');
    let (path, blob, mtime, mode) = (
        fields.next()?,
        fields.next()?,
        fields.next()?,
        fields.next()?,
    );
    if fields.next().is_some() || path.is_empty() || blob.is_empty() || mode.is_empty() {
        return None;
    }
    Some(SavedInput {
        file: TrackedFile {
            path: path.to_owned(),
            blob: blob.to_owned(),
            mode: mode.to_owned(),
        },
        mtime_ns: mtime.parse().ok()?,
    })
}

#[must_use]
pub fn plan_restore(saved: &[SavedInput], tracked: &Tracked) -> RestorePlan {
    let (saved_links, saved_files): (Vec<&SavedInput>, Vec<&SavedInput>) = saved
        .iter()
        .partition(|input| input.file.mode == SYMLINK_MODE);
    let saved_links: BTreeSet<&TrackedFile> = saved_links.iter().map(|input| &input.file).collect();
    let current_links: BTreeSet<&TrackedFile> = tracked.links.iter().collect();
    let links_changed = saved_links
        .symmetric_difference(&current_links)
        .map(|link| link.path.as_str())
        .collect::<BTreeSet<_>>()
        .len()
        + tracked.unverifiable_links;
    if links_changed > 0 {
        return RestorePlan {
            links_changed,
            ..RestorePlan::default()
        };
    }
    let saved_by_path: BTreeMap<&str, &SavedInput> = saved_files
        .into_iter()
        .map(|input| (input.file.path.as_str(), input))
        .collect();
    let current: BTreeSet<&str> = tracked
        .files
        .iter()
        .map(|file| file.path.as_str())
        .chain(tracked.absent.iter().map(String::as_str))
        .collect();
    let mut plan = RestorePlan {
        deleted: saved_by_path
            .keys()
            .filter(|path| !current.contains(*path))
            .count(),
        absent: tracked.absent.len(),
        unsupported: tracked.unsupported,
        ..RestorePlan::default()
    };
    for file in &tracked.files {
        match saved_by_path.get(file.path.as_str()) {
            Some(input) if input.file == *file => plan.restore.push(SavedInput {
                file: file.clone(),
                mtime_ns: input.mtime_ns,
            }),
            Some(_) => plan.changed += 1,
            None => plan.new += 1,
        }
    }
    plan
}
