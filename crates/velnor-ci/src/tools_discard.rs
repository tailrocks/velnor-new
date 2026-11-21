// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use crate::error::CiError;

const MARK: &str = "velnor-tools-discarded";

pub fn mark(runner_temp: &Path) -> Result<(), CiError> {
    let path = runner_temp.join(MARK);
    fs::write(&path, "").map_err(|error| {
        CiError::new(format!(
            "cannot record the discard in {}: {error}",
            path.display()
        ))
    })
}

#[must_use]
pub fn pending(runner_temp: &Path) -> bool {
    fs::symlink_metadata(runner_temp.join(MARK)).is_ok_and(|metadata| metadata.is_file())
}

pub fn clear(runner_temp: &Path) {
    fs::remove_file(runner_temp.join(MARK)).ok();
}
