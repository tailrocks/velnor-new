// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use velnor_model::{RepoPath, lf_normalized};

use super::{Error, program};

pub(super) fn stored_bytes(
    root: &Path,
    files: &mut BTreeMap<RepoPath, Vec<u8>>,
    listed: &[RepoPath],
) -> Result<(), Error> {
    for path in listed {
        let Some(bytes) = files.get_mut(path) else {
            continue;
        };
        if !bytes.contains(&b'\r') {
            continue;
        }
        let spec = OsString::from(format!(":{path}"));
        let blob = program(
            root,
            "git",
            &[OsStr::new("cat-file"), OsStr::new("blob"), &spec],
        )
        .ok()
        .filter(|blob| lf_normalized(blob) == lf_normalized(bytes))
        .ok_or_else(|| Error::StoredBytes(path.clone()))?;
        *bytes = blob;
    }
    Ok(())
}
