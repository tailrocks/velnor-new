// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use velnor_model::{RepoPath, RepoView};
use velnor_rust::{METADATA_PATH, recorded_metadata, wanted_files};

pub(super) struct ReportCheckout {
    pub checkout: velnor_gen::io::Checkout,
    pub symlink_locks: BTreeSet<RepoPath>,
}

fn ci(error: impl std::fmt::Display) -> CiError {
    CiError::new(error.to_string())
}

pub(super) fn read(directory: &Path) -> Result<ReportCheckout, CiError> {
    let tracked = super::inputs::tracked_paths(directory)?;
    let tracked_paths = tracked
        .paths
        .into_iter()
        .map(RepoPath::new)
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(ci)?;
    let symlinks: BTreeSet<_> = tracked_paths
        .iter()
        .filter(|path| {
            fs::symlink_metadata(tracked.top.join(path.as_str()))
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
        })
        .cloned()
        .collect();
    let symlink_locks: BTreeSet<_> = symlinks
        .iter()
        .filter(|path| {
            Path::new(path.as_str())
                .file_name()
                .is_some_and(|name| name == "Cargo.lock")
        })
        .cloned()
        .collect();
    let wanted: Vec<_> = wanted_files(&tracked_paths)
        .into_iter()
        .filter(|path| {
            !symlink_locks.contains(path)
                && !(symlinks.contains(path) && path.as_str().ends_with(".rs"))
        })
        .collect();
    let mut view = RepoView {
        repository: velnor_gen::io::repository_name(&tracked.top),
        files: velnor_gen::io::read_files(&tracked.top, &wanted).map_err(ci)?,
        tracked_paths,
        symlinks,
        rust_metadata: BTreeMap::new(),
    };
    velnor_gen::io::add_manifests(&mut view, |paths| {
        velnor_gen::io::read_files(&tracked.top, paths)
    })
    .map_err(ci)?;
    let mut recorded = velnor_gen::io::read_files(&tracked.top, &[METADATA_PATH]).map_err(ci)?;
    view.rust_metadata =
        recorded_metadata(&view, recorded.remove(&METADATA_PATH).as_deref()).map_err(ci)?;
    Ok(ReportCheckout {
        checkout: velnor_gen::io::Checkout {
            root: tracked.top,
            view,
        },
        symlink_locks,
    })
}
