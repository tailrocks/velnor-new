// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{CollectorError, filesystem};

pub(super) fn verify(expected: &[u8], inventory: &filesystem::Tree) -> Result<(), CollectorError> {
    let actual = filesystem::tree_hash(inventory);
    if expected != actual {
        return Err(CollectorError::mismatch(
            "golden manifest does not match the complete cases inventory",
        ));
    }
    Ok(())
}
