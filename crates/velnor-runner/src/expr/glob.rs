// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use globset::{GlobBuilder, GlobMatcher};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) struct OrderedGlobs {
    matchers: Vec<(bool, GlobMatcher)>,
}

impl OrderedGlobs {
    pub(crate) fn new(patterns: &[String]) -> Result<Self, globset::Error> {
        let matchers = patterns
            .iter()
            .map(|pattern| {
                let (negative, pattern) = pattern
                    .strip_prefix('!')
                    .map_or((false, pattern.as_str()), |pattern| (true, pattern));
                GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .build()
                    .map(|glob| (negative, glob.compile_matcher()))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { matchers })
    }

    pub(crate) fn is_match(&self, path: &str) -> bool {
        self.matchers
            .iter()
            .fold(false, |matched, (negative, matcher)| {
                if matcher.is_match(path) {
                    !negative
                } else {
                    matched
                }
            })
    }
}

fn static_prefix(workspace: &Path, pattern: &str) -> Option<PathBuf> {
    let pattern = pattern.strip_prefix('!').unwrap_or(pattern);
    if pattern.is_empty() {
        return None;
    }
    let mut root = workspace.to_path_buf();
    for segment in pattern.split('/') {
        if segment.contains(['*', '?', '[', ']', '{', '}']) {
            break;
        }
        if segment == ".." {
            return None;
        }
        if !segment.is_empty() && segment != "." {
            root.push(segment);
        }
    }
    Some(root)
}

pub(crate) fn search_roots(workspace: &Path, patterns: &[String]) -> Vec<PathBuf> {
    let candidates: Vec<PathBuf> = patterns
        .iter()
        .filter_map(|pattern| static_prefix(workspace, pattern))
        .collect();
    let candidate_set: BTreeSet<PathBuf> = candidates.iter().cloned().collect();
    let mut selected = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|candidate| {
            !candidate
                .ancestors()
                .skip(1)
                .any(|ancestor| candidate_set.contains(ancestor))
                && selected.insert(candidate.clone())
        })
        .collect()
}

enum Kind {
    File,
    Dir,
    Other,
}

fn kind_of(path: &Path, file_type: fs::FileType) -> Kind {
    if file_type.is_dir() {
        Kind::Dir
    } else if file_type.is_file() {
        Kind::File
    } else if file_type.is_symlink() {
        match fs::metadata(path) {
            Ok(target) if target.is_file() => Kind::File,
            Ok(_) => Kind::Other,
            Err(_) => Kind::File,
        }
    } else {
        Kind::Other
    }
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        match kind_of(&path, file_type) {
            Kind::File => files.push(path),
            Kind::Dir => walk(&path, files),
            Kind::Other => {}
        }
    }
}

pub(crate) fn collect_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let kind = fs::symlink_metadata(root)
        .map_or(Kind::Other, |metadata| kind_of(root, metadata.file_type()));
    match kind {
        Kind::File => files.push(root.to_path_buf()),
        Kind::Dir => {
            walk(root, &mut files);
            files.sort();
        }
        Kind::Other => {}
    }
    files
}
