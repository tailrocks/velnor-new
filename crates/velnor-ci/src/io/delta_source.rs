// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use velnor_model::RepoPath;

use crate::error::CiError;
use crate::inputs::Tracked;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceSnapshot {
    pub identity: String,
    pub tracked: Tracked,
}

impl SourceSnapshot {
    pub(super) fn records(&self) -> Result<BTreeMap<RepoPath, (&str, &str, &str)>, CiError> {
        let invalid = || CiError::new("delta source snapshot is incomplete or invalid");
        if self.tracked.submodules != 0 || identity_of(&self.tracked)? != self.identity {
            return Err(invalid());
        }
        let mut records = BTreeMap::new();
        for (kind, files) in [("file", &self.tracked.files), ("link", &self.tracked.links)] {
            for file in files {
                let valid = if kind == "file" {
                    matches!(file.blob.len(), 40 | 64)
                        && file
                            .blob
                            .bytes()
                            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
                        && u32::from_str_radix(&file.mode, 8)
                            .is_ok_and(|mode| mode & !0o7777 == 0o100_000)
                } else {
                    file.mode == crate::inputs::SYMLINK_MODE
                        && crate::inputs::is_representable(&file.blob)
                };
                let path = RepoPath::new(&file.path).map_err(|_| invalid())?;
                if !valid
                    || records
                        .insert(path, (kind, file.blob.as_str(), file.mode.as_str()))
                        .is_some()
                {
                    return Err(invalid());
                }
            }
        }
        for path in &self.tracked.absent {
            let path = RepoPath::new(path).map_err(|_| invalid())?;
            if records.insert(path, ("absent", "", "")).is_some() {
                return Err(invalid());
            }
        }
        Ok(records)
    }
}

pub(super) fn identity(root: &Path) -> Result<String, CiError> {
    identity_of(&super::inputs::source_files(root)?)
}

pub(super) fn identity_of(tracked: &Tracked) -> Result<String, CiError> {
    if tracked.unsupported > 0 || tracked.unverifiable_links > 0 {
        return Err(CiError::new("tracked source identity is unavailable"));
    }
    let mut digest = Sha256::new();
    for (kind, files) in [("file", &tracked.files), ("link", &tracked.links)] {
        let mut files: Vec<_> = files.iter().collect();
        files.sort();
        for file in files {
            for field in [kind, &file.path, &file.blob, &file.mode] {
                field_digest(&mut digest, field.as_bytes());
            }
        }
    }
    let mut absent: Vec<_> = tracked.absent.iter().collect();
    absent.sort();
    for path in absent {
        field_digest(&mut digest, b"absent");
        field_digest(&mut digest, path.as_bytes());
    }
    field_digest(&mut digest, b"submodules");
    field_digest(&mut digest, &(tracked.submodules as u64).to_le_bytes());
    let hex = b"0123456789abcdef";
    Ok(digest
        .finalize()
        .iter()
        .flat_map(|byte| [hex[usize::from(byte >> 4)], hex[usize::from(byte & 0xf)]])
        .map(char::from)
        .collect())
}

fn field_digest(digest: &mut Sha256, field: &[u8]) {
    digest.update((field.len() as u64).to_le_bytes());
    digest.update(field);
}
