// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use super::super::{CollectorError, filesystem};

pub(super) fn verify(golden_dir: &Path, cases_root: &Path) -> Result<(), CollectorError> {
    let manifest_path = golden_dir.join("MANIFEST.sha256");
    match fs::symlink_metadata(&manifest_path) {
        Ok(metadata) if metadata.file_type().is_file() => {}
        Ok(_) => {
            return Err(CollectorError::operational(
                "golden manifest must be a regular non-symlink file",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(CollectorError::mismatch("golden manifest is missing"));
        }
        Err(error) => return Err(CollectorError::operational(error)),
    }
    let expected = fs::read(&manifest_path).map_err(CollectorError::operational)?;
    let inventory = filesystem::read_tree(cases_root)?;
    let actual = filesystem::tree_hash(&inventory);
    if expected != actual {
        return Err(CollectorError::mismatch(
            "golden manifest does not match the complete cases inventory",
        ));
    }
    Ok(())
}
