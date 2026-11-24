// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use velnor_model::RustRoot;

pub(in super::super) fn invoked_root<'a>(
    roots: &'a [RustRoot],
    repository: &Path,
    location: &Path,
) -> Option<&'a RustRoot> {
    roots
        .iter()
        .filter_map(|root| {
            claim(root, repository, location).map(|claim| {
                (
                    claim,
                    root_directory(root, repository).components().count(),
                    root,
                )
            })
        })
        .max_by_key(|(claim, depth, _)| (*claim, *depth))
        .map(|(_, _, root)| root)
}

fn claim(root: &RustRoot, repository: &Path, location: &Path) -> Option<usize> {
    std::iter::once(root_directory(root, repository))
        .chain(root.members.iter().filter_map(|member| {
            member
                .manifest
                .parent()
                .map(|parent| repository.join(parent.as_str()))
        }))
        .filter(|directory| location.starts_with(directory))
        .map(|directory| directory.components().count())
        .max()
}

pub(in super::super) fn root_directory(root: &RustRoot, repository: &Path) -> PathBuf {
    root.manifest.parent().map_or_else(
        || repository.to_path_buf(),
        |parent| repository.join(parent.as_str()),
    )
}
