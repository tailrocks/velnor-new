// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

pub const TIE_STEP_NS: u64 = 1_000;

#[must_use]
pub fn break_ties(files: &[(String, u64)]) -> Vec<(String, u64)> {
    let mut taken: BTreeSet<u64> = files.iter().map(|(_, time)| *time).collect();
    let mut groups: BTreeMap<u64, Vec<&str>> = BTreeMap::new();
    for (path, time) in files {
        groups.entry(*time).or_default().push(path);
    }
    let mut moved = Vec::new();
    for (time, mut paths) in groups {
        paths.sort_unstable();
        for path in paths.into_iter().skip(1) {
            let mut candidate = time;
            loop {
                candidate = candidate.saturating_sub(TIE_STEP_NS);
                if taken.insert(candidate) || candidate == 0 {
                    break;
                }
            }
            moved.push((path.to_owned(), candidate));
        }
    }
    moved.sort();
    moved
}
