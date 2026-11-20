// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::super::super::units::{UnitFile, Workspace, unit_files};
use super::super::groups::scan_dirs;
use super::super::inventory::{self, Evidence, State};
use super::{identity, output};
use crate::error::CiError;

pub(super) fn proven(workspaces: &[Workspace]) -> Result<BTreeSet<PathBuf>, CiError> {
    let base = inventory::exclusions(workspaces);
    let mut found = BTreeSet::new();
    for target in scan_dirs(workspaces) {
        for (unit, path) in unit_files(&target)? {
            if output(&unit)
                && let Ok(saved) = inventory::load(&path)
                && base.is_subset(&saved.targets)
            {
                found.extend(saved.targets.iter().cloned());
            }
        }
    }
    Ok(found)
}

pub(super) fn targets(
    workspaces: &[Workspace],
    evidence: &Evidence,
    before: &BTreeMap<PathBuf, (String, SystemTime)>,
) -> Result<BTreeSet<PathBuf>, CiError> {
    let mut found = BTreeSet::new();
    for target in scan_dirs(workspaces) {
        for (unit, path) in unit_files(&target)? {
            let (UnitFile::Fingerprint { configuration, .. }
            | UnitFile::BuildOutput { configuration, .. }
            | UnitFile::DepInfo {
                configuration: Some(configuration),
                ..
            }) = unit
            else {
                continue;
            };
            let mut components = Path::new(&configuration).components();
            let Some(child) = components.next().filter(|_| components.next().is_some()) else {
                continue;
            };
            let directory = fs::canonicalize(&target)
                .map_err(|error| CiError::new(error.to_string()))?
                .join(child);
            if evidence.state(&directory) == Some(State::Missing)
                && before.get(&path) != Some(&identity(&path)?)
            {
                found.insert(directory);
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod parses;
