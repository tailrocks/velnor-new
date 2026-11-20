// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::units::{UnitFile, unit_files};
use crate::error::CiError;
use crate::inputs::restored_units_file;

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

pub(super) fn read(target: &Path, prefix: &str) -> Option<BTreeSet<String>> {
    let text = fs::read_to_string(target.join(restored_units_file(prefix))).ok()?;
    crate::restored_units::decode(&text)
}
