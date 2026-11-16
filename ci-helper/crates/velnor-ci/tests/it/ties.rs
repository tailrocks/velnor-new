// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs::File;
use std::time::{Duration, Instant, UNIX_EPOCH};

use velnor_ci::error::CiError;
use velnor_ci::io::break_time_ties_with;
use velnor_ci::ties::{TIE_STEP_NS, break_ties};

use crate::io::Repo;

fn reference(files: &[(String, u64)]) -> Vec<(String, u64)> {
    use std::collections::BTreeMap;
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
            let mut found = None;
            while candidate > 0 {
                candidate = candidate.saturating_sub(TIE_STEP_NS);
                if !taken.contains(&candidate) {
                    found = Some(candidate);
                    break;
                }
            }
            let found = found.or_else(|| (0..time).rev().find(|e| !taken.contains(e)));
            let candidate = found.unwrap_or_else(|| unreachable!("reference ran out"));
            taken.insert(candidate);
            moved.push((path.to_owned(), candidate));
        }
    }
    moved.sort();
    moved
}

#[test]
fn a_hundred_thousand_tied_files_are_given_distinct_times_quickly() {
    let files: Vec<(String, u64)> = (0..100_000)
        .map(|n| (format!("f{n:06}"), 1_700_000_000_000_000_000))
        .collect();
    let started = Instant::now();
    let moved = break_ties(&files).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    let times: BTreeSet<u64> = moved.iter().map(|(_, time)| *time).collect();
    assert_eq!(moved.len(), 99_999);
    assert_eq!(times.len(), 99_999);
}

#[test]
fn small_tie_groups_get_exactly_the_times_the_per_file_rescan_gave() {
    let t = 9_000_000;
    let mut files: Vec<(String, u64)> = Vec::new();
    for (group, time) in [t, t - TIE_STEP_NS, t - 2 * TIE_STEP_NS, 2_500, 700]
        .into_iter()
        .enumerate()
    {
        for n in 0..(3 + group) {
            files.push((format!("g{group}-{n}"), time));
        }
    }
    let moved = break_ties(&files);
    assert_eq!(moved.ok(), Some(reference(&files)));
}

#[test]
fn a_filesystem_that_rounds_distinct_times_back_into_a_tie_fails_the_restore() {
    let repo = Repo::new("rounding");
    for path in ["build.rs", "src/lib.rs"] {
        repo.write(path, "x\n");
    }
    let tied = UNIX_EPOCH + Duration::new(1_700_000_000, 0);
    for path in [".gitignore", "build.rs", "src/lib.rs"] {
        repo.set_mtime(path, tied);
    }
    let round_to_seconds = |path: &std::path::Path, ns: u64| -> Result<(), CiError> {
        let time = UNIX_EPOCH + Duration::from_secs(ns / 1_000_000_000);
        File::options()
            .write(true)
            .open(path)
            .and_then(|file| file.set_times(std::fs::FileTimes::new().set_modified(time)))
            .map_err(|e| CiError::new(e.to_string()))
    };
    let error = break_time_ties_with(repo.root(), &round_to_seconds).err();
    let text = error.map(|e| e.to_string()).unwrap_or_default();
    assert!(
        text.contains("still share a modification time") && text.contains("build.rs"),
        "{text}"
    );
}
