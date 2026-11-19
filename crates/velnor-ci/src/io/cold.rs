// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::inputs::tracked_paths;
use crate::error::CiError;

pub fn cold_on_failure(
    targets: &[PathBuf],
    what: &str,
    run: impl FnOnce() -> Result<String, CiError>,
) -> Result<String, CiError> {
    let reason = match run() {
        Ok(report) => return Ok(report),
        Err(e) => e,
    };
    for target in targets {
        match fs::remove_dir_all(target) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => {
                return Err(CiError::new(format!(
                    "{what} failed ({reason}) and {} cannot be removed: {e}",
                    target.display()
                )));
            }
        }
    }
    Ok(format!(
        "{what} failed: {reason}; removed {} cached target directories and building cold\n",
        targets.len()
    ))
}

#[must_use]
pub fn root_target(root: &Path) -> Vec<PathBuf> {
    vec![root.join("target")]
}

pub fn every_root_target(root: &Path) -> Result<Vec<PathBuf>, CiError> {
    let mut targets = root_target(root);
    let listed = tracked_paths(root).map_err(|e| {
        CiError::new(format!(
            "cannot list the tracked Cargo manifests, so the cached target directories cannot all be found and cleared: {e}"
        ))
    })?;
    for path in listed.paths {
        if let Some(dir) = path.strip_suffix("Cargo.toml")
            && (dir.is_empty() || dir.ends_with('/'))
        {
            let target = listed.top.join(dir).join("target");
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
    }
    Ok(targets)
}
