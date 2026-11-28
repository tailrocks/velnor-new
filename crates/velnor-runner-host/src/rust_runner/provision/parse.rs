// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    pub(super) uid: u32,
    pub(super) gid: u32,
    pub(super) mode: u32,
    pub(super) kind: String,
}

impl Entry {
    pub(super) fn is(&self, kind: &str, mode: u32) -> bool {
        self.uid == 0 && self.gid == 0 && self.mode == mode && self.kind == kind
    }

    pub(super) fn is_closed_root_directory(&self) -> bool {
        self.uid == 0 && self.kind == "directory" && self.mode & 0o022 == 0
    }
}

pub(super) fn entries(output: &str) -> HashMap<String, Entry> {
    output.lines().filter_map(entry).collect()
}

fn entry(line: &str) -> Option<(String, Entry)> {
    let mut fields = line.splitn(5, ' ');
    let path = fields.next()?.to_owned();
    let uid = fields.next()?.parse().ok()?;
    let gid = fields.next()?.parse().ok()?;
    let mode = u32::from_str_radix(fields.next()?, 8).ok()?;
    let kind = fields.next()?.trim().to_owned();
    Some((
        path,
        Entry {
            uid,
            gid,
            mode,
            kind,
        },
    ))
}

pub(super) fn lines(output: &str) -> Vec<&str> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

pub(super) fn digest(output: &str) -> Option<String> {
    let token = output.split_whitespace().next()?;
    (token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| token.to_ascii_lowercase())
}

pub(super) fn free_bytes(output: &str) -> Option<u64> {
    let (_, rest) = output
        .lines()
        .find_map(|line| line.trim().split_once("Free (estimated):"))?;
    rest.split_whitespace().next()?.parse().ok()
}
