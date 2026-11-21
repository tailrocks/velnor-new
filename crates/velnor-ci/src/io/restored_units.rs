// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
    for (unit, path) in units {
        match unit {
            UnitFile::Fingerprint { hash, .. } => {
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
    Some(found)
}

fn names_missing_file<'a>(dep_info: &Path, inside: impl Iterator<Item = &'a PathBuf>) -> bool {
    let Some(text) = fs::metadata(dep_info)
        .ok()
        .filter(|metadata| metadata.len() <= DEP_INFO_LIMIT)
        .and_then(|_| fs::read_to_string(dep_info).ok())
    else {
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
