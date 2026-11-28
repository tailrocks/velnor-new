// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::units::{UnitFile, unit_files};
use crate::error::CiError;
use crate::inputs::restored_units_file;
use crate::restored_units::{Fingerprint, Manifest, RestoredUnits};

const DEP_INFO_LIMIT: u64 = 4 * 1024 * 1024;

pub(super) fn scan(target: &Path) -> Option<RestoredUnits> {
    let units = unit_files(target).ok()?;
    let inside = fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    let mut found = RestoredUnits::default();
    let mut fingerprints = Vec::new();
    for (unit, path) in units {
        match unit {
            UnitFile::Fingerprint {
                hash,
                package,
                name,
                ..
            } => {
                fingerprints.push(Fingerprint {
                    hash: hash.clone(),
                    package,
                    unit: name,
                    dependencies: dependency_crates(&path),
                });
                found.restored.insert(hash.clone());
                found.present.insert(hash);
            }
            UnitFile::DepInfo {
                hash: Some(hash), ..
            } => {
                found.restored.insert(hash.clone());
                if names_missing_file(&path, &inside) {
                    found.missing_input.insert(hash);
                }
            }
            _ => {}
        }
    }
    let missing = found.missing_input.clone();
    found.present.retain(|hash| !missing.contains(hash));
    crate::restored_units::cascade(&mut found, &fingerprints);
    Some(found)
}

fn dependency_crates(fingerprint: &Path) -> BTreeSet<String> {
    let Some(text) = read_small(fingerprint) else {
        return BTreeSet::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return BTreeSet::new();
    };
    value
        .get("deps")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|dependency| dependency.get(1)?.as_str().map(str::to_owned))
        .collect()
}

fn read_small(file: &Path) -> Option<String> {
    fs::metadata(file)
        .ok()
        .filter(|metadata| metadata.len() <= DEP_INFO_LIMIT)
        .and_then(|_| fs::read_to_string(file).ok())
}

fn names_missing_file(dep_info: &Path, inside: &Path) -> bool {
    let Some(text) = read_small(dep_info) else {
        return false;
    };
    velnor_rust::dep_info_paths(&text).iter().any(|named| {
        let named = Path::new(named);
        named.is_absolute()
            && !physical(named).starts_with(inside)
            && !cargo_unpacks(named)
            && fs::metadata(named).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    })
}

fn physical(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut ancestor = path;
    loop {
        if let Ok(resolved) = fs::canonicalize(ancestor) {
            return rest
                .iter()
                .rev()
                .fold(resolved, |joined, part| joined.join(part));
        }
        let (Some(parent), Some(name)) = (ancestor.parent(), ancestor.file_name()) else {
            return path.to_path_buf();
        };
        rest.push(name);
        ancestor = parent;
    }
}

fn cargo_unpacks(path: &Path) -> bool {
    let parts: Vec<_> = path
        .components()
        .map(std::path::Component::as_os_str)
        .collect();
    parts.windows(2).any(|pair| {
        (pair[0] == "registry" && pair[1] == "src") || (pair[0] == "git" && pair[1] == "checkouts")
    })
}

pub(super) fn record(
    target: &Path,
    prefix: &str,
    units: Option<&RestoredUnits>,
) -> Result<(), CiError> {
    let Some(units) = units else {
        return Ok(());
    };
    let file = target.join(restored_units_file(prefix));
    fs::write(&file, crate::restored_units::encode(units))
        .map_err(|e| CiError::new(format!("cannot record restored units: {e}")))
}

pub(super) fn read(target: &Path, prefix: &str) -> (Manifest, Option<RestoredUnits>) {
    match fs::read_to_string(target.join(restored_units_file(prefix))) {
        Ok(text) => match crate::restored_units::decode(&text) {
            Some(units) => (Manifest::Present, Some(units)),
            None => (Manifest::Unreadable, None),
        },
        Err(error) if error.kind() == ErrorKind::NotFound => (Manifest::Absent, None),
        Err(_) => (Manifest::Unreadable, None),
    }
}
