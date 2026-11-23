// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use velnor_rust::sha256_hex;

use super::super::super::units::UnitFile;
use crate::error::CiError;

pub(super) fn output(unit: &UnitFile) -> bool {
    matches!(
        unit,
        UnitFile::BuildOutput { .. } | UnitFile::DepInfo { .. } | UnitFile::Script { .. }
    )
}

pub(super) fn identity(path: &Path) -> Result<(String, SystemTime), CiError> {
    let bytes = fs::read(path).map_err(|e| CiError::new(e.to_string()))?;
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|e| CiError::new(e.to_string()))?;
    Ok((sha256_hex(&bytes), modified))
}
