// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Read as _;
use std::path::Path;

pub(crate) fn path_is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

pub(crate) fn read_staged_bytes(path: &Path, bound: u64) -> Result<Vec<u8>, &'static str> {
    read_staged_bytes_until(path, bound, || Ok(()))
}

pub(crate) fn read_staged_bytes_until(
    path: &Path,
    bound: u64,
    mut checkpoint: impl FnMut() -> Result<(), &'static str>,
) -> Result<Vec<u8>, &'static str> {
    checkpoint()?;
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|err| {
        if err == rustix::io::Errno::NOENT {
            "missing"
        } else if err == rustix::io::Errno::LOOP {
            "symlink"
        } else {
            "unreadable"
        }
    })?;
    let filetype = rustix::fs::fstat(&fd)
        .map(|stat| rustix::fs::FileType::from_raw_mode(stat.st_mode))
        .map_err(|_| "unreadable")?;
    if filetype != rustix::fs::FileType::RegularFile {
        return Err("unreadable");
    }
    let mut file = fs::File::from(fd).take(bound.saturating_add(1));
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        checkpoint()?;
        let remaining = bound
            .saturating_add(1)
            .saturating_sub(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        if remaining == 0 {
            return Err("oversize");
        }
        let limit = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let count = file.read(&mut buffer[..limit]).map_err(|_| "unreadable")?;
        checkpoint()?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > bound {
            return Err("oversize");
        }
    }
    Ok(bytes)
}

pub(crate) fn read_staged_text(path: &Path, bound: u64) -> Result<String, &'static str> {
    let bytes = read_staged_bytes(path, bound)?;
    String::from_utf8(bytes).map_err(|_| "unreadable")
}
