//! Shared staged-file reads: `NOFOLLOW` open, handle validation, size bound.
//!
//! Declared via `#[path]` from `retrieve_reports.rs` (no `lib.rs`
//! edit); `retrieve_reports` re-exports the gate so staged-report
//! reads, merge-request assembly, and baseline entry reads share the
//! one implementation.

use std::fs;
use std::io::Read as _;
use std::path::Path;

/// True when a traversed directory is a symlink.
///
/// `symlink_metadata` never follows the final component: a symlink
/// rejects even at a live target. Missing paths are not links; the
/// bounded read below reports them as missing instead.
pub(crate) fn path_is_symlink(path: &Path) -> bool {
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
pub(crate) fn read_staged_bytes(path: &Path, bound: u64) -> Result<Vec<u8>, &'static str> {
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
    let mut bytes = Vec::new();
    fs::File::from(fd)
        .take(bound.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "unreadable")?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > bound {
        return Err("oversize");
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
pub(crate) fn read_staged_text(path: &Path, bound: u64) -> Result<String, &'static str> {
    let bytes = read_staged_bytes(path, bound)?;
    String::from_utf8(bytes).map_err(|_| "unreadable")
}
