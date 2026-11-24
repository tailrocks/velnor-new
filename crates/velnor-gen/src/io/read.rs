// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use velnor_model::{RepoPath, RepoView};
use velnor_rust::{
    build_script_sources, is_rust_source, manifests, metadata_roots, reduce_metadata, wanted_files,
};

use super::check::cargo_metadata;
use super::error::Error;
use super::identity;
use super::tracked::tracked;

type Recorded = (RepoView, BTreeMap<RepoPath, Vec<u8>>);

fn view_from(
    root: &Path,
    tracked_paths: BTreeSet<RepoPath>,
    symlinks: BTreeSet<RepoPath>,
    gitlinks: BTreeSet<RepoPath>,
) -> Result<RepoView, Error> {
    let wanted: Vec<RepoPath> = wanted_files(&tracked_paths)
        .into_iter()
        .filter(|path| !(symlinks.contains(path) && is_rust_source(path)))
        .collect();
    Ok(RepoView {
        repository: identity::repository_name(root),
        files: read_files(root, &wanted)?,
        tracked_paths,
        symlinks,
        gitlinks,
        rust_metadata: BTreeMap::new(),
    })
}

pub(super) fn read_tracked(root: &Path) -> Result<RepoView, Error> {
    let (tracked_paths, symlinks, gitlinks) = tracked(root)?;
    view_from(root, tracked_paths, symlinks, gitlinks)
}

pub(super) fn read_view(root: &Path, keep_metadata: bool) -> Result<Recorded, Error> {
    let (tracked_paths, symlinks, gitlinks) = tracked(root)?;
    view_of(root, tracked_paths, symlinks, gitlinks, keep_metadata)
}

pub fn view_of(
    root: &Path,
    tracked_paths: BTreeSet<RepoPath>,
    symlinks: BTreeSet<RepoPath>,
    gitlinks: BTreeSet<RepoPath>,
    keep_metadata: bool,
) -> Result<Recorded, Error> {
    let mut view = view_from(root, tracked_paths, symlinks, gitlinks)?;
    let mut recorded = BTreeMap::new();
    let roots = metadata_roots(&view).map_err(Error::Detect)?;
    for manifest in roots {
        let known = view.rust_metadata.iter().any(|(workspace, metadata)| {
            *workspace == manifest || metadata.members.iter().any(|m| m.manifest == manifest)
        });
        if known {
            continue;
        }
        let output = cargo_metadata(root, &manifest)?;
        let (workspace, metadata) = reduce_metadata(root, &output).map_err(Error::Metadata)?;
        if keep_metadata {
            recorded.insert(workspace.clone(), output);
        }
        view.rust_metadata.insert(workspace, metadata);
    }
    let roots: Vec<RepoPath> = view.rust_metadata.keys().cloned().collect();
    view.files.extend(read_files(root, &roots)?);
    for source in build_script_sources(&view) {
        view.files
            .extend(read_files(root, std::slice::from_ref(&source))?);
    }
    add_manifests(&mut view, |paths| read_files(root, paths))?;
    Ok((view, recorded))
}

pub fn add_manifests(
    view: &mut RepoView,
    read: impl FnOnce(&[RepoPath]) -> Result<BTreeMap<RepoPath, Vec<u8>>, Error>,
) -> Result<(), Error> {
    for (manifest, bytes) in read(&manifests(&view.tracked_paths))? {
        view.files.entry(manifest).or_insert(bytes);
    }
    Ok(())
}

pub fn read_files(root: &Path, paths: &[RepoPath]) -> Result<BTreeMap<RepoPath, Vec<u8>>, Error> {
    let mut files = BTreeMap::new();
    for path in paths {
        if let Some(file) = regular_file(root, path)? {
            let bytes = fs::read(&file).map_err(|e| Error::File(file, e))?;
            files.insert(path.clone(), bytes);
        }
    }
    Ok(files)
}

pub(super) fn regular_file(root: &Path, path: &RepoPath) -> Result<Option<PathBuf>, Error> {
    let parts: Vec<&str> = path.as_str().split('/').collect();
    let mut current = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        current.push(part);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(Error::File(current, error)),
        };
        let last = index + 1 == parts.len();
        if (last && !metadata.is_file()) || (!last && !metadata.is_dir()) {
            return Err(Error::NotRegular(path.clone()));
        }
    }
    Ok(Some(current))
}
