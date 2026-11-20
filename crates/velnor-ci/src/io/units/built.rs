// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A target directory that holds the output of a build but no unit file in a layout this helper
//! reads. Reading it as "no units" would let every verdict that rests on the units (the audit,
//! the inputs lock, the build-script inventories) pass over nothing.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::UnitFile;
use crate::error::CiError;

/// A file only a build leaves: a compiled library, its metadata, dep-info, or a unit's marker.
pub(super) fn output(name: &OsStr) -> bool {
    let path = Path::new(name);
    path.extension()
        .is_some_and(|extension| matches!(extension.to_str(), Some("rlib" | "rmeta" | "d")))
        || name == "invoked.timestamp"
}

pub(super) fn refuse_without_units(
    target: &Path,
    built: bool,
    found: &[(UnitFile, PathBuf)],
) -> Result<(), CiError> {
    let unit = |file: &UnitFile| !matches!(file, UnitFile::Directory { .. });
    if built && !found.iter().any(|(file, _)| unit(file)) {
        return Err(CiError::new(format!(
            "found no units in {}; unknown cargo layout?",
            target.display()
        )));
    }
    Ok(())
}
