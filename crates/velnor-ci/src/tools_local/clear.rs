// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use super::HOME_PATHS;
use crate::tree_removal::remove_tree;

const UNSAFE_PATH: &str = "a tool directory is a symlink or leaves HOME, so nothing is cleared";
const UNSAFE_HOME: &str = "HOME is not an absolute directory below the root, so nothing is cleared";
const NOT_REMOVED: &str = "a tool directory could not be removed";

fn present(home: &Path, relative: &str) -> Result<bool, &'static str> {
    let mut path = home.to_path_buf();
    for part in Path::new(relative).components() {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(UNSAFE_PATH),
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(UNSAFE_PATH),
        }
    }
    Ok(true)
}

pub(super) fn clear_home(home: &Path) -> Result<(), &'static str> {
    if !home.is_absolute() || fs::canonicalize(home).map_or(true, |real| real.parent().is_none()) {
        return Err(UNSAFE_HOME);
    }
    let mut owned = Vec::new();
    for path in HOME_PATHS {
        if present(home, path)? {
            owned.push(home.join(path));
        }
    }
    owned
        .iter()
        .try_for_each(|target| remove_tree(target))
        .map_err(|_| NOT_REMOVED)
}
