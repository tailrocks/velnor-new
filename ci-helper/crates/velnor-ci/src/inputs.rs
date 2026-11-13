// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::error::CiError;

pub const INPUTS_FILE: &str = "target/.velnor/inputs.tsv";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedFile {
    pub path: String,
    pub blob: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedInput {
    pub path: String,
    pub blob: String,
    pub mtime_ns: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tracked {
    pub files: Vec<TrackedFile>,
    pub modified: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestorePlan {
    pub restore: Vec<SavedInput>,
    pub changed: usize,
    pub new: usize,
    pub modified: usize,
}

#[must_use]
pub fn parse_ls_files(staged: &[u8], modified: &[u8]) -> Tracked {
    let modified: BTreeSet<&[u8]> = nul_records(modified).collect();
    let mut tracked = Tracked::default();
    for file in nul_records(staged)
        .filter_map(|record| std::str::from_utf8(record).ok())
        .filter_map(parse_ls_files_record)
    {
        if modified.contains(file.path.as_bytes()) {
            tracked.modified += 1;
        } else {
            tracked.files.push(file);
        }
    }
    tracked
}

fn nul_records(output: &[u8]) -> impl Iterator<Item = &[u8]> {
    output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
}

fn parse_ls_files_record(record: &str) -> Option<TrackedFile> {
    let (meta, path) = record.split_once('\t')?;
    let mut fields = meta.split(' ');
    let (mode, blob, stage) = (fields.next()?, fields.next()?, fields.next()?);
    let regular_file = mode == "100644" || mode == "100755";
    (regular_file && stage == "0" && representable(path)).then(|| TrackedFile {
        path: path.to_owned(),
        blob: blob.to_owned(),
    })
}

fn representable(path: &str) -> bool {
    !path.is_empty() && !path.contains(['\t', '\n', '\r'])
}

#[must_use]
pub fn format_inputs(inputs: &[SavedInput]) -> String {
    let mut text = String::new();
    for input in inputs {
        text.push_str(&input.path);
        text.push('\t');
        text.push_str(&input.blob);
        text.push('\t');
        text.push_str(&input.mtime_ns.to_string());
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
                    "{INPUTS_FILE} line {} is not `path<TAB>blob<TAB>mtime_ns`: `{line}`",
                    index + 1
                ))
            })
        })
        .collect()
}

fn parse_inputs_line(line: &str) -> Option<SavedInput> {
    let mut fields = line.split('\t');
    let (path, blob, mtime) = (fields.next()?, fields.next()?, fields.next()?);
    if fields.next().is_some() || path.is_empty() || blob.is_empty() {
        return None;
    }
    Some(SavedInput {
        path: path.to_owned(),
        blob: blob.to_owned(),
        mtime_ns: mtime.parse().ok()?,
    })
}

#[must_use]
pub fn plan_restore(saved: &[SavedInput], tracked: &Tracked) -> RestorePlan {
    let saved: BTreeMap<&str, &SavedInput> = saved
        .iter()
        .map(|input| (input.path.as_str(), input))
        .collect();
    let mut plan = RestorePlan {
        modified: tracked.modified,
        ..RestorePlan::default()
    };
    for file in &tracked.files {
        match saved.get(file.path.as_str()) {
            Some(input) if input.blob == file.blob => plan.restore.push(SavedInput {
                path: file.path.clone(),
                blob: file.blob.clone(),
                mtime_ns: input.mtime_ns,
            }),
            Some(_) => plan.changed += 1,
            None => plan.new += 1,
        }
    }
    plan
}
