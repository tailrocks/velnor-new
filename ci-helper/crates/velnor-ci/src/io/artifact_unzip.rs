// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::io::{self, Cursor, Read};
use std::path::Path;

use crate::error::CiError;

const MEMBER_LIMIT: u64 = 64 << 20;
const TOTAL_LIMIT: u64 = 256 << 20;
const MEMBER_COUNT: usize = 10_000;

pub(super) fn extract(bytes: &[u8], name: &str, reports: &Path) -> Result<(), CiError> {
    let staging = reports.join(format!(".{name}.partial"));
    let destination = reports.join(name);
    remove(&staging)?;
    let written = write_members(bytes, &staging).and_then(|()| {
        remove(&destination)?;
        fs::rename(&staging, &destination)
            .map_err(|e| CiError::new(format!("cannot move {}: {e}", staging.display())))
    });
    if written.is_err() {
        let _cleanup = remove(&staging);
    }
    written
}

fn remove(path: &Path) -> Result<(), CiError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CiError::new(format!(
            "cannot remove {}: {error}",
            path.display()
        ))),
    }
}

fn write_members(bytes: &[u8], staging: &Path) -> Result<(), CiError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| CiError::new(format!("not a zip archive: {e}")))?;
    if archive.len() > MEMBER_COUNT {
        return Err(CiError::new(format!(
            "archive has more than {MEMBER_COUNT} members"
        )));
    }
    fs::create_dir_all(staging)
        .map_err(|e| CiError::new(format!("cannot create {}: {e}", staging.display())))?;
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let mut member = archive
            .by_index(index)
            .map_err(|e| CiError::new(format!("unreadable archive member {index}: {e}")))?;
        let relative = member.enclosed_name().ok_or_else(|| {
            CiError::new(format!(
                "archive member {index} names a path outside the report directory"
            ))
        })?;
        if member.is_symlink() {
            return Err(CiError::new(format!(
                "archive member {} is a symbolic link",
                relative.display()
            )));
        }
        let target = staging.join(&relative);
        if member.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|e| CiError::new(format!("cannot create {}: {e}", target.display())))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| CiError::new(format!("cannot create {}: {e}", parent.display())))?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| CiError::new(format!("cannot create {}: {e}", target.display())))?;
        let copied = io::copy(&mut (&mut member).take(MEMBER_LIMIT + 1), &mut file)
            .map_err(|e| CiError::new(format!("cannot extract {}: {e}", relative.display())))?;
        total += copied;
        if copied > MEMBER_LIMIT || total > TOTAL_LIMIT {
            return Err(CiError::new(format!(
                "archive member {} is larger than the extraction limit",
                relative.display()
            )));
        }
    }
    Ok(())
}
