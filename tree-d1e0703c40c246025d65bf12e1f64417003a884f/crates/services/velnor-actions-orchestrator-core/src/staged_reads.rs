//! Shared staged-file reads: `NOFOLLOW` open, handle validation, size bound.
//!
//! Staged-report reads, merge-request assembly, baseline entry reads,
//! and check preparation share this one implementation.

use std::fs;
use std::io::Read as _;
use std::path::Path;

/// True when a traversed directory is a symlink.
///
/// `symlink_metadata` never follows the final component: a symlink
/// rejects even at a live target. Missing paths are not links; the
/// bounded read below reports them as missing instead.
pub fn path_is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// Read one file's bytes with symlink rejection and a caller size bound.
///
/// Opens with `O_NOFOLLOW`, validates the open handle (`fstat` must
/// report a regular file), and reads only through that handle with a
/// `bound + 1` cap, so a symlink swapped in after any pre-check still
/// fails the open instead of diverting the read. Missing files report
/// `missing`, symlinks `symlink`, non-files `unreadable`, and
/// over-bound reads `oversize`.
pub fn read_staged_bytes(path: &Path, bound: u64) -> Result<Vec<u8>, &'static str> {
    read_staged_bytes_until(path, bound, || Ok(()))
}

/// Read staged bytes in bounded chunks, checking a caller's shared deadline.
pub fn read_staged_bytes_until(
    path: &Path,
    bound: u64,
    mut checkpoint: impl FnMut() -> Result<(), &'static str>,
) -> Result<Vec<u8>, &'static str> {
    checkpoint()?;
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
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

/// Read one file with symlink rejection and a caller size bound.
///
/// Shared by staged-report reads, merge-request assembly, and baseline
/// entry reads so every event-time read enforces the same gates:
/// symlinks and non-files reject, missing files report, and oversize
/// files error instead of exhausting memory. Bytes decode as UTF-8;
/// undecodable files report `unreadable`.
pub fn read_staged_text(path: &Path, bound: u64) -> Result<String, &'static str> {
    let bytes = read_staged_bytes(path, bound)?;
    String::from_utf8(bytes).map_err(|_| "unreadable")
}
