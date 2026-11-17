// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use super::delta::{failure, safe_path};
use super::delta_source::SourceSnapshot;
use crate::error::CiError;

pub(super) fn invalidate(target: &Path, saved: &SourceSnapshot) -> Result<usize, CiError> {
    let directory = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let root = fs::canonicalize(directory).map_err(|e| failure(directory, &e))?;
    let text =
        fs::read_to_string(target.join(".velnor/inputs.tsv")).map_err(|e| failure(target, &e))?;
    if super::input_snapshot::decode(&text)?.source_identity != saved.identity {
        return Err(CiError::new(
            "delta sources differ from the saved input identity",
        ));
    }
    let before = saved.records()?;
    let tracked = super::inputs::source_files(&root)?;
    let current = SourceSnapshot {
        identity: super::delta_source::identity_of(&tracked)?,
        tracked,
    };
    if before == current.records()? {
        return Ok(0);
    }
    let fingerprints = fingerprints(target)?;
    let removed = fingerprints.len();
    for path in fingerprints {
        safe_path(&path)?;
        super::delta_tree::remove_node(&path)?;
    }
    Ok(removed)
}

fn fingerprints(target: &Path) -> Result<Vec<PathBuf>, CiError> {
    let mut result = Vec::new();
    for path in super::delta_tree::paths(target)? {
        if path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == ".fingerprint")
        {
            let full = target.join(&path);
            if fs::symlink_metadata(&full)
                .map_err(|e| failure(&full, &e))?
                .is_dir()
            {
                result.push(full);
            }
        }
    }
    Ok(result)
}
