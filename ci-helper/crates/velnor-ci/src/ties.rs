// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::error::CiError;

pub const TIE_STEP_NS: u64 = 1_000;

pub fn break_ties(files: &[(String, u64)]) -> Result<Vec<(String, u64)>, CiError> {
    let mut taken: BTreeSet<u64> = files.iter().map(|(_, time)| *time).collect();
    let mut groups: BTreeMap<u64, Vec<&str>> = BTreeMap::new();
    for (path, time) in files {
        groups.entry(*time).or_default().push(path);
    }
    let mut moved = Vec::new();
    for (time, mut paths) in groups {
        paths.sort_unstable();
        for path in paths.into_iter().skip(1) {
            let candidate = free_time(&taken, time).ok_or_else(|| {
                CiError::new(format!(
                    "cannot give {path} a modification time earlier than {time} ns that no other tracked file has"
                ))
            })?;
            taken.insert(candidate);
            moved.push((path.to_owned(), candidate));
        }
    }
    moved.sort();
    Ok(moved)
}

fn free_time(taken: &BTreeSet<u64>, time: u64) -> Option<u64> {
    let mut candidate = time;
    while candidate > 0 {
        candidate = candidate.saturating_sub(TIE_STEP_NS);
        if !taken.contains(&candidate) {
            return Some(candidate);
        }
    }
    (0..time).rev().find(|earlier| !taken.contains(earlier))
}
