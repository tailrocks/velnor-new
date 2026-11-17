// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

const OWN_FOLDERS: [&str; 4] = ["src/", "tests/", "benches/", "examples/"];
const BUILD_SCRIPT: &str = "build.rs";

pub(crate) fn claims(directory: Option<&RepoPath>, whole: bool, path: &RepoPath) -> bool {
    relative(directory, path).is_some_and(|rest| {
        whole || rest == BUILD_SCRIPT || OWN_FOLDERS.iter().any(|folder| rest.starts_with(folder))
    })
}

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
