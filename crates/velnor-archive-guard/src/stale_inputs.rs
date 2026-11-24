// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io;
use std::path::Path;

pub fn verify_unwatched_inputs(root: &str, unwatched: &str) -> Result<(), String> {
    unwatched
        .split(' ')
        .filter(|relative| !relative.is_empty())
        .try_for_each(|relative| {
            match fs::symlink_metadata(Path::new(root).join(relative)) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                _ => Err(format!(
                    "embedded source fingerprint is stale: {relative} was not watched and appeared after this build; run cargo clean -p velnor-archive-guard"
                )),
            }
        })
}
