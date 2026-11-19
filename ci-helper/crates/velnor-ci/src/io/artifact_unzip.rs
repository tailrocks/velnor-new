// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::io::{self, Cursor, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::CiError;

const MEMBER_LIMIT: u64 = 64 << 20;
const MEMBER_COUNT: usize = 10_000;

/// The bytes every artifact of one fetch may extract to together; workers draw on it as they write.
#[derive(Debug)]
pub(super) struct Budget(AtomicU64);

impl Budget {
    pub(super) fn new(bytes: u64) -> Self {
        Self(AtomicU64::new(bytes))
    }

    fn take(&self, bytes: u64) -> bool {
        self.0
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.checked_sub(bytes)
            })
            .is_ok()
    }

    fn give_back(&self, bytes: u64) {
        self.0.fetch_add(bytes, Ordering::Relaxed);
    }
}

pub(super) fn extract(
    bytes: &[u8],
    name: &str,
    reports: &Path,
    budget: &Budget,
) -> Result<(), CiError> {
    let staging = reports.join(format!(".{name}.partial"));
    let destination = reports.join(name);
    remove(&staging)?;
    let mut drawn = 0;
    let written = write_members(bytes, &staging, budget, &mut drawn).and_then(|()| {
        remove(&destination)?;
        fs::rename(&staging, &destination)
            .map_err(|e| CiError::new(format!("cannot move {}: {e}", staging.display())))
    });
    if written.is_err() {
        let _cleanup = remove(&staging);
        budget.give_back(drawn);
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

fn write_members(
    bytes: &[u8],
    staging: &Path,
    budget: &Budget,
    drawn: &mut u64,
) -> Result<(), CiError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| CiError::new(format!("not a zip archive: {e}")))?;
    if archive.len() > MEMBER_COUNT {
        return Err(CiError::new(format!(
            "archive has more than {MEMBER_COUNT} members"
        )));
    }
    fs::create_dir_all(staging)
        .map_err(|e| CiError::new(format!("cannot create {}: {e}", staging.display())))?;
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
        let copied = copy_member(
            &mut (&mut member).take(MEMBER_LIMIT + 1),
            &mut file,
            budget,
            drawn,
        )
        .map_err(|e| CiError::new(format!("cannot extract {}: {e}", relative.display())))?;
        if copied > MEMBER_LIMIT {
            return Err(CiError::new(format!(
                "archive member {} is larger than the extraction limit",
                relative.display()
            )));
        }
    }
    Ok(())
}

fn copy_member(
    from: &mut impl Read,
    to: &mut impl Write,
    budget: &Budget,
    drawn: &mut u64,
) -> io::Result<u64> {
    let mut chunk = [0_u8; 64 << 10];
    let mut copied = 0_u64;
    loop {
        let read = from.read(&mut chunk)?;
        if read == 0 {
            return Ok(copied);
        }
        let read = read as u64;
        if !budget.take(read) {
            return Err(io::Error::other(
                "the extraction budget for all report artifacts is used up",
            ));
        }
        *drawn += read;
        to.write_all(&chunk[..read as usize])?;
        copied += read;
    }
}
