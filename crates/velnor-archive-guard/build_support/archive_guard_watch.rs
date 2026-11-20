// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) struct Plan {
    pub(crate) rerun: Vec<PathBuf>,
    pub(crate) unwatched: Vec<String>,
}

pub(crate) fn plan(
    watched: Vec<PathBuf>,
    absent: Vec<(String, PathBuf)>,
    out_dir: Option<&Path>,
) -> Plan {
    let unwatchable = |source: &Path| out_dir.is_some_and(|out| walk_may_reach(source, out));
    let unwatched = absent
        .into_iter()
        .filter(|(_, nearest)| unwatchable(nearest))
        .map(|(relative, _)| relative)
        .collect();
    let rerun = watched
        .into_iter()
        .filter(|source| !unwatchable(source))
        .collect();
    Plan { rerun, unwatched }
}

fn walk_may_reach(source: &Path, out_dir: &Path) -> bool {
    if out_dir.starts_with(source) {
        return true;
    }
    let (Ok(real_source), Ok(real_out)) = (source.canonicalize(), out_dir.canonicalize()) else {
        return true;
    };
    real_out.starts_with(&real_source) || links_reach(&real_source, &real_out)
}

fn links_reach(directory: &Path, real_out: &Path) -> bool {
    if !directory.is_dir() {
        return false;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return true;
    };
    entries.into_iter().any(|entry| {
        let Ok(entry) = entry else {
            return true;
        };
        let path = entry.path();
        let is_link =
            fs::symlink_metadata(&path).map_or(true, |meta| meta.file_type().is_symlink());
        is_link
            && path
                .canonicalize()
                .map_or(true, |real| real_out.starts_with(real))
    })
}
