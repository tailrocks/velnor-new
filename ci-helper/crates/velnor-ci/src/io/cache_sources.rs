// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::error::CiError;

pub(super) fn refusal(root: &Path) -> Result<Option<String>, CiError> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            "Cargo.lock",
            "**/Cargo.lock",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| CiError::new(format!("cannot list Cargo.lock files: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(
            "cannot list Cargo.lock files for cache eligibility",
        ));
    }
    let listing = String::from_utf8(output.stdout)
        .map_err(|_| CiError::new("Cargo.lock path is not UTF-8"))?;
    let paths: Vec<&str> = listing.split_terminator('\0').collect();
    if paths.is_empty() {
        return Ok(Some(
            "base caches are not saved because no Cargo.lock proves dependency sources public\n"
                .to_owned(),
        ));
    }
    for path in paths {
        let public = fs::read_to_string(root.join(path))
            .ok()
            .and_then(|text| velnor_rust::cacheable_lock(&text).ok())
            .unwrap_or(false);
        if !public {
            return Ok(Some(format!(
                "base caches are not saved because dependency sources in {path} are not proven public\n"
            )));
        }
    }
    Ok(None)
}
