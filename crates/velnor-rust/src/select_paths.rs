// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

pub(super) fn depth(directory: Option<&RepoPath>) -> usize {
    directory.map_or(0, |directory| directory.as_str().len())
}

pub(crate) fn relative<'p>(directory: Option<&RepoPath>, path: &'p RepoPath) -> Option<&'p str> {
    match directory {
        None => Some(path.as_str()),
        Some(directory) => path
            .as_str()
            .strip_prefix(directory.as_str())?
            .strip_prefix('/'),
    }
}

pub(super) fn contains(directory: Option<&RepoPath>, path: &RepoPath) -> bool {
    relative(directory, path).is_some()
}
