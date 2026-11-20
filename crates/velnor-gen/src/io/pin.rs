// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use velnor_rust::{
    LOCK_PATH, binary_sums_url, commit_url, lists_binary_sums, pin_lock, release_version, sums_url,
};

use crate::require_compatible;

use super::{Error, program, regular_file, replace};

pub(super) fn pin_helper(root: &Path, version: &str) -> Result<String, Error> {
    let version = release_version(version).map_err(Error::Lock)?;
    require_compatible(&version).map_err(Error::Generate)?;
    let root = lock_root(root)?;
    let sums = download(&root, &sums_url(&version))?;
    let commit = download(&root, &commit_url(&version))?;
    let sums = String::from_utf8_lossy(&sums);
    let binary_sums = if lists_binary_sums(&sums) {
        Some(download(&root, &binary_sums_url(&version))?)
    } else {
        None
    };
    let lock = pin_lock(
        version.as_str(),
        &String::from_utf8_lossy(&commit),
        &sums,
        binary_sums
            .as_deref()
            .map(String::from_utf8_lossy)
            .as_deref(),
    )
    .map_err(Error::Lock)?;
    write_lock(&root, &lock)?;
    if root.join(".git").exists() {
        program(
            &root,
            "git",
            &[
                OsStr::new("add"),
                OsStr::new("--"),
                OsStr::new(LOCK_PATH.as_str()),
            ],
        )?;
        return Ok(format!(
            "pinned velnor-ci and velnor-gen {version} in {LOCK_PATH} and staged it"
        ));
    }
    Ok(format!(
        "pinned velnor-ci and velnor-gen {version} in {}",
        root.join(LOCK_PATH.as_str()).display()
    ))
}

/// The directory that holds `.velnor/`: `--root` as given. The git repository that encloses it
/// is not searched for, so a directory inside another repository keeps its own lock and nothing
/// is written or staged in that repository.
pub fn lock_root(root: &Path) -> Result<PathBuf, Error> {
    fs::canonicalize(root).map_err(|e| Error::File(root.into(), e))
}

fn download(root: &Path, url: &str) -> Result<Vec<u8>, Error> {
    program(
        root,
        "curl",
        &[
            OsStr::new("--fail"),
            OsStr::new("--silent"),
            OsStr::new("--show-error"),
            OsStr::new("--location"),
            OsStr::new(url),
        ],
    )
}

pub fn write_lock(root: &Path, lock: &str) -> Result<(), Error> {
    regular_file(root, &LOCK_PATH)?;
    replace(&root.join(LOCK_PATH.as_str()), lock.as_bytes())
}
