// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use super::model::{BUNDLE, RECEIPT};
use crate::error::CiError;
use crate::preview::json::{Res, require};

type Identity = (u64, u64, u64, i64, i64);

fn identity(file: &File) -> Res<Identity> {
    let meta = file
        .metadata()
        .map_err(|e| CiError::new(format!("source preview file: {e}")))?;
    require(meta.file_type().is_file())?;
    Ok((
        meta.dev(),
        meta.ino(),
        meta.size(),
        meta.mtime(),
        meta.mtime_nsec(),
    ))
}

fn read_exact_file(path: &Path, limit: u64) -> Res<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| CiError::new(format!("source preview file: {e}")))?;
    let before = identity(&file)?;
    require(before.2 > 0 && before.2 <= limit)?;
    let mut data = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut data)
        .map_err(|e| CiError::new(format!("source preview file: {e}")))?;
    let after = identity(&file)?;
    require(data.len() as u64 == before.2 && before == after)?;
    Ok(data)
}

fn names_of(directory: &Path) -> Res<BTreeSet<String>> {
    let meta = fs::symlink_metadata(directory)
        .map_err(|e| CiError::new(format!("source preview directory: {e}")))?;
    require(meta.file_type().is_dir())?;
    fs::read_dir(directory)
        .map_err(|e| CiError::new(format!("source preview directory: {e}")))?
        .map(|entry| {
            entry
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .map_err(|e| CiError::new(format!("source preview directory: {e}")))
        })
        .collect()
}

/// Port of `read_file`: the one file `name` of `directory`, which holds nothing else.
pub fn read_file(directory: &Path, name: &str, limit: u64) -> Res<Vec<u8>> {
    require(names_of(directory)? == BTreeSet::from([name.to_owned()]))?;
    read_exact_file(&directory.join(name), limit)
}

/// Port of `read_pair`: the receipt and the bundle of a signed pair directory.
pub fn read_pair(directory: &Path) -> Res<(Vec<u8>, Vec<u8>)> {
    require(names_of(directory)? == BTreeSet::from([RECEIPT.to_owned(), BUNDLE.to_owned()]))?;
    Ok((
        read_exact_file(&directory.join(RECEIPT), 65_536)?,
        read_exact_file(&directory.join(BUNDLE), crate::preview::MAX_JSON as u64)?,
    ))
}

/// Create `path`, which must not exist, with `data`.
pub fn create_new(path: &Path, data: &[u8]) -> Res<()> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(data))
        .map_err(|e| CiError::new(format!("source preview file: {e}")))
}

/// Append to the file `GITHUB_OUTPUT` names.
pub fn append(path: &Path, text: &str) -> Res<()> {
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|e| CiError::new(format!("source preview output: {e}")))
}
