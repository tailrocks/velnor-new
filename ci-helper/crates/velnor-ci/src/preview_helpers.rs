// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;

use crate::error::CiError;

pub const ARTIFACT_ID_VARIABLE: &str = "VELNOR_EVIDENCE_ARTIFACT_ID";
pub const BUNDLE_NAME: &str = "SDK_PREVIEW_RECEIPT.json.sigstore.jsonl";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_ID_LENGTH: usize = 16;

pub fn validate_artifact_id(id: &str) -> Result<(), CiError> {
    let canonical = id
        .as_bytes()
        .first()
        .is_some_and(|first| (b'1'..=b'9').contains(first))
        && id.bytes().all(|byte| byte.is_ascii_digit());
    if !canonical {
        return Err(CiError::new(format!(
            "artifact id {id:?} is not a positive decimal integer without leading zeros"
        )));
    }
    if id.len() > MAX_ID_LENGTH {
        return Err(CiError::new(format!(
            "artifact id {id} has more than {MAX_ID_LENGTH} digits"
        )));
    }
    match id.parse::<u64>() {
        Ok(value) if value <= MAX_SAFE_INTEGER => Ok(()),
        _ => Err(CiError::new(format!(
            "artifact id {id} exceeds {MAX_SAFE_INTEGER}"
        ))),
    }
}

pub fn install_gh(version: &str, token: Option<&str>) -> Result<(), CiError> {
    if token.is_none_or(str::is_empty) {
        return Err(CiError::new("missing workflow token: set GITHUB_TOKEN"));
    }
    let status = Command::new("mise")
        .args(["--no-config", "--no-env", "--no-hooks", "install"])
        .arg(format!("gh@{version}"))
        .status()
        .map_err(|e| CiError::new(format!("cannot run mise: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(CiError::new(format!(
            "mise install gh@{version} failed: {status}"
        )))
    }
}

pub fn stage_bundle(bundle: &Path, runner_temp: &Path) -> Result<(), CiError> {
    let link = fs::symlink_metadata(bundle)
        .map_err(|e| CiError::new(format!("cannot stat bundle {}: {e}", bundle.display())))?;
    if link.file_type().is_symlink() {
        return Err(CiError::new(format!(
            "bundle {} is a symlink",
            bundle.display()
        )));
    }
    if !link.is_file() {
        return Err(CiError::new(format!(
            "bundle {} is not a regular file",
            bundle.display()
        )));
    }
    if link.len() == 0 {
        return Err(CiError::new(format!(
            "bundle {} is empty",
            bundle.display()
        )));
    }
    let bytes = fs::read(bundle)
        .map_err(|e| CiError::new(format!("cannot read bundle {}: {e}", bundle.display())))?;
    let destination = runner_temp.join(BUNDLE_NAME);
    let write = |mut file: fs::File| file.write_all(&bytes).and_then(|()| file.sync_all());
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .open(&destination)
        .and_then(write)
        .map_err(|e: io::Error| {
            CiError::new(format!("cannot stage {}: {e}", destination.display()))
        })
}
