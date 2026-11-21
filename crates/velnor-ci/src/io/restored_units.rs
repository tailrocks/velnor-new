// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use super::units::{UnitFile, unit_files};
use crate::error::CiError;
use crate::inputs::restored_units_file;
use crate::restored_units::Manifest;

pub(super) fn scan(target: &Path) -> Option<BTreeSet<String>> {
    unit_files(target).ok().map(|units| {
        units
            .into_iter()
            .filter_map(|(unit, _)| match unit {
                UnitFile::Fingerprint { hash, .. } => Some(hash),
                _ => None,
            })
            .collect()
    })
}

pub(super) fn record(
    target: &Path,
    prefix: &str,
    hashes: Option<&BTreeSet<String>>,
) -> Result<(), CiError> {
    let Some(hashes) = hashes else {
        return Ok(());
    };
    let file = target.join(restored_units_file(prefix));
    fs::write(&file, crate::restored_units::encode(hashes))
        .map_err(|e| CiError::new(format!("cannot record restored units: {e}")))
}

pub(super) fn read(target: &Path, prefix: &str) -> (Manifest, Option<BTreeSet<String>>) {
    match fs::read_to_string(target.join(restored_units_file(prefix))) {
        Ok(text) => match crate::restored_units::decode(&text) {
            Some(hashes) => (Manifest::Present, Some(hashes)),
            None => (Manifest::Unreadable, None),
        },
        Err(error) if error.kind() == ErrorKind::NotFound => (Manifest::Absent, None),
        Err(_) => (Manifest::Unreadable, None),
    }
}
