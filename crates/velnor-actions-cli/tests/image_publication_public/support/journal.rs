// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use std::{error::Error, fs, path::Path};

pub(crate) fn read_receipt(path: &Path) -> Result<Value, Box<dyn Error>> {
    let contents = fs::read_to_string(path)?;
    let lines = contents.lines().collect::<Vec<_>>();
    if lines.len() != 2 {
        return Err("expected pending and verified receipt records".into());
    }
    let pending: Value = serde_json::from_str(lines[0])?;
    let verified: Value = serde_json::from_str(lines[1])?;
    assert_eq!(pending["schema"], "velnor.image-publication-pending.v1");
    assert_eq!(verified["schema"], "velnor.image-publication.v1");
    assert_eq!(pending["status"], "pending");
    assert_eq!(verified["status"], "verified");
    assert_eq!(pending["profile"], verified["profile"]);
    assert_eq!(pending["source_commit"], verified["source_commit"]);
    assert_eq!(pending["repository"], verified["repository"]);
    assert_eq!(pending["tag"], verified["tag"]);
    assert_eq!(pending["expected_digest"], verified["registry_digest"]);
    assert_eq!(pending["archive_sha256"], verified["archive_sha256"]);
    assert_eq!(pending["record_sha256"], verified["record_sha256"]);
    Ok(verified)
}

pub(crate) fn read_pending(path: &Path) -> Result<Value, Box<dyn Error>> {
    let contents = fs::read_to_string(path)?;
    let lines = contents.lines().collect::<Vec<_>>();
    if lines.len() != 1 {
        return Err("expected only pending receipt record".into());
    }
    let pending: Value = serde_json::from_str(lines[0])?;
    assert_eq!(pending["schema"], "velnor.image-publication-pending.v1");
    assert_eq!(pending["status"], "pending");
    Ok(pending)
}
