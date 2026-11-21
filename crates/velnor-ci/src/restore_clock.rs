// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use crate::error::CiError;
use crate::time::utc_text;

pub const SAFETY_MARGIN_SECONDS: i64 = 300;

const BEGAN: &str = "velnor-tools-restore-began";

pub fn record(runner_temp: &Path, now: i64) -> Result<(), CiError> {
    let path = runner_temp.join(BEGAN);
    fs::write(&path, now.to_string()).map_err(|error| {
        CiError::new(format!(
            "cannot record when the restore began in {}: {error}",
            path.display()
        ))
    })
}

fn recorded(runner_temp: &Path) -> Option<i64> {
    let path = runner_temp.join(BEGAN);
    fs::symlink_metadata(&path)
        .ok()
        .filter(fs::Metadata::is_file)?;
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[must_use]
pub fn cutoff(runner_temp: &Path, now: i64) -> String {
    let began = recorded(runner_temp).map_or(now, |began| began.min(now));
    utc_text(began - SAFETY_MARGIN_SECONDS)
}
