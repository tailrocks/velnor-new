// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::super::inventory::STORE;
use super::invocation::Selection;
use crate::error::CiError;

const MARKER: &str = "failed-inputs-build.json";

#[derive(Serialize, Deserialize)]
pub(super) struct Failure {
    pub(super) command: String,
    pub(super) selection: Selection,
}

fn marker(directory: &Path) -> PathBuf {
    directory.join(STORE).join(MARKER)
}

pub(super) fn record_failure(directories: &[PathBuf], failure: &Failure) -> Result<(), CiError> {
    let bytes = serde_json::to_vec(failure).map_err(|e| CiError::new(e.to_string()))?;
    for directory in directories {
        let path = marker(directory);
        fs::create_dir_all(directory.join(STORE))
            .and_then(|()| fs::write(&path, &bytes))
            .map_err(|e| {
                CiError::new(format!(
                    "cannot record failed build in {}: {e}",
                    path.display()
                ))
            })?;
    }
    Ok(())
}

pub(super) fn clear_failure(directories: &[PathBuf]) -> Result<(), CiError> {
    for directory in directories {
        let path = marker(directory);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CiError::new(format!(
                    "cannot clear {}: {error}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn earlier_failure(
    directories: &[PathBuf],
    packages: &BTreeSet<&str>,
) -> Option<String> {
    directories.iter().find_map(|directory| {
        let bytes = fs::read(marker(directory)).ok()?;
        let failure: Failure = serde_json::from_slice(&bytes).ok()?;
        packages
            .iter()
            .any(|package| failure.selection.selects(package))
            .then_some(failure.command)
    })
}
