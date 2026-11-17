// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::input_target::{check_removal_target, check_target};
use super::inputs::tracked_paths;
use super::restored_targets::{self, Pending};
use crate::error::CiError;
use velnor_rust::{METADATA_PATH, detect};

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
        check_removal_target(target)?;
    }
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

pub fn every_root_target(root: &Path) -> Result<Vec<PathBuf>, CiError> {
    Ok(targets_with_receipts(root)?.0)
}

pub(super) fn targets_with_receipts(root: &Path) -> Result<(Vec<PathBuf>, Pending), CiError> {
    let pending = restored_targets::read(root)?;
    let listed = tracked_paths(root)?;
    let mut targets = if listed
        .top
        .join(METADATA_PATH.as_str())
        .try_exists()
        .map_err(|e| CiError::new(format!("cannot find {METADATA_PATH}: {e}")))?
    {
        let checkout = velnor_gen::io::read_recorded_checkout(&listed.top)
            .map_err(|e| CiError::new(e.to_string()))?;
        let facts = detect(&checkout.view).map_err(|e| CiError::new(e.to_string()))?;
        let mut targets = Vec::new();
        for root in facts.rust_roots {
            let target = checkout.root.join(root.target_dir.as_str());
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        targets
    } else {
        let mut targets = vec![root.join("target")];
        for path in listed.paths {
            if path.file_name().is_some_and(|name| name == "Cargo.toml")
                && let Some(dir) = path.parent()
            {
                let target = listed.top.join(dir).join("target");
                if !targets.contains(&target) {
                    targets.push(target);
                }
            }
        }
        targets
    };
    for target in &pending.targets {
        if !targets.contains(target) {
            targets.push(target.clone());
        }
    }
    for target in &targets {
        check_target(root, target)?;
    }
    Ok((targets, pending))
}

pub(super) fn explicit_targets_with_receipts(
    root: &Path,
    targets: &[PathBuf],
) -> Result<(Vec<PathBuf>, Pending), CiError> {
    let pending = restored_targets::read(root)?;
    let mut targets: Vec<_> = targets.iter().map(|target| root.join(target)).collect();
    targets.extend(pending.targets.iter().cloned());
    targets.sort();
    targets.dedup();
    for target in &targets {
        check_target(root, target)?;
    }
    Ok((targets, pending))
}
