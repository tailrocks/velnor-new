// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::CiError;

pub(super) fn identity(root: &Path) -> Result<String, CiError> {
    let mut tracked = super::inputs::source_files(root)?;
    if tracked.unsupported > 0 || tracked.unverifiable_links > 0 {
        return Err(CiError::new("tracked source identity is unavailable"));
    }
    tracked.files.sort();
    tracked.links.sort();
    tracked.absent.sort();
    let mut digest = Sha256::new();
    for (kind, files) in [("file", tracked.files), ("link", tracked.links)] {
        for file in files {
            for field in [kind, &file.path, &file.blob, &file.mode] {
                field_digest(&mut digest, field.as_bytes());
            }
        }
    }
    for path in tracked.absent {
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

pub(super) fn unchanged(root: &Path) -> bool {
    let Ok(expected) = std::env::var("EXPECTED_SOURCE_IDENTITY") else {
        return false;
    };
    expected.len() == 64
        && expected
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && identity(root).is_ok_and(|actual| actual == expected)
}
