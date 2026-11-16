// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::inputs::tracked_paths;
use crate::error::CiError;

pub(super) fn check_target(root: &Path, target: &Path) -> Result<(), CiError> {
    let target = resolve(&root.join(target))?;
    let root = resolve(root)?;
    let tracked = tracked_paths(&root)?;
    if root.starts_with(&target)
        || tracked
            .paths
            .iter()
            .any(|path| tracked.top.join(path).starts_with(&target))
    {
        return Err(CiError::new(format!(
            "target directory {} contains checkout files",
            target.display()
        )));
    }
    Ok(())
}

fn resolve(path: &Path) -> Result<PathBuf, CiError> {
    match fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let parent = path.parent().zip(path.file_name()).ok_or_else(|| {
                CiError::new(format!(
                    "cannot resolve target directory {}: {e}",
                    path.display()
                ))
            })?;
            Ok(resolve(parent.0)?.join(parent.1))
        }
        Err(e) => Err(CiError::new(format!(
            "cannot resolve target directory {}: {e}",
            path.display()
        ))),
    }
}
