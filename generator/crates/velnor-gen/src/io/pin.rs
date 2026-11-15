// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::path::Path;

use velnor_rust::{LOCK_PATH, pin_lock, release_version, sums_url};

use crate::require_compatible;

use super::{Error, program, regular_file, replace, toplevel};

pub(super) fn pin_helper(root: &Path, version: &str) -> Result<String, Error> {
    let version = release_version(version).map_err(Error::Lock)?;
    require_compatible(&version).map_err(Error::Generate)?;
    let root = toplevel(root)?;
    let url = sums_url(&version);
    let sums = program(
        &root,
        "curl",
        &[
            OsStr::new("--fail"),
            OsStr::new("--silent"),
            OsStr::new("--show-error"),
            OsStr::new("--location"),
            OsStr::new(&url),
        ],
    )?;
    let lock = pin_lock(version.as_str(), &String::from_utf8_lossy(&sums)).map_err(Error::Lock)?;
    write_lock(&root, &lock)?;
    program(
        &root,
        "git",
        &[
            OsStr::new("add"),
            OsStr::new("--"),
            OsStr::new(LOCK_PATH.as_str()),
        ],
    )?;
    Ok(format!(
        "pinned velnor-ci and velnor-gen {version} in {LOCK_PATH} and staged it"
    ))
}

pub fn write_lock(root: &Path, lock: &str) -> Result<(), Error> {
    regular_file(root, &LOCK_PATH)?;
    replace(&root.join(LOCK_PATH.as_str()), lock.as_bytes())
}
