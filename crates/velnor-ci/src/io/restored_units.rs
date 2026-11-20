// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::units::{UnitFile, unit_files};
use crate::error::CiError;
use crate::inputs::restored_units_file;

/// Writes the hashes of the units `target` holds, in either layout, to the restored-unit record.
/// A scan that fails writes nothing: with no record the report keeps its stricter rule.
pub(super) fn record(target: &Path, prefix: &str) -> Result<(), CiError> {
    let file = target.join(restored_units_file(prefix));
    let Ok(units) = unit_files(target) else {
        return Ok(());
    };
    let hashes: BTreeSet<String> = units
        .into_iter()
        .filter_map(|(unit, _)| match unit {
            UnitFile::Fingerprint { hash, .. } => Some(hash),
            _ => None,
        })
        .collect();
    fs::write(&file, crate::restored_units::encode(&hashes))
        .map_err(|e| CiError::new(format!("cannot record restored units: {e}")))
}

/// The record `record` wrote, or `None` when there is none or it cannot be read.
pub(super) fn read(target: &Path, prefix: &str) -> Option<BTreeSet<String>> {
    let text = fs::read_to_string(target.join(restored_units_file(prefix))).ok()?;
    crate::restored_units::decode(&text)
}
