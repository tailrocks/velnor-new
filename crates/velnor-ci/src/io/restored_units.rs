// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::units::{UnitFile, unit_files};
use crate::error::CiError;
use crate::inputs::restored_units_file;
use crate::restored_units::RestoredUnits;

const DEP_INFO_LIMIT: u64 = 4 * 1024 * 1024;

pub(super) fn scan(target: &Path) -> Option<RestoredUnits> {
    let units = unit_files(target).ok()?;
    let inside = [Some(target.to_path_buf()), fs::canonicalize(target).ok()];
    let mut found = RestoredUnits::default();
    let mut fingerprints = Vec::new();
    for (unit, path) in units {
        match unit {
            UnitFile::Fingerprint { hash, package, .. } => {
                fingerprints.push((hash.clone(), package, dependency_crates(&path)));
                found.present.insert(hash);
            }
            UnitFile::DepInfo {
                hash: Some(hash), ..
            } if names_missing_file(&path, inside.iter().flatten()) => {
                found.missing_input.insert(hash);
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

fn names_missing_file<'a>(dep_info: &Path, inside: impl Iterator<Item = &'a PathBuf>) -> bool {
    let Some(text) = read_small(dep_info) else {
        return false;
    };
    let inside: Vec<&PathBuf> = inside.collect();
    velnor_rust::dep_info_paths(&text).iter().any(|named| {
        let named = Path::new(named);
        named.is_absolute()
            && !inside.iter().any(|root| named.starts_with(root))
            && !cargo_unpacks(named)
            && fs::metadata(named).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    })
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

pub(super) fn read(target: &Path, prefix: &str) -> Option<RestoredUnits> {
    let text = fs::read_to_string(target.join(restored_units_file(prefix))).ok()?;
    crate::restored_units::decode(&text)
}
