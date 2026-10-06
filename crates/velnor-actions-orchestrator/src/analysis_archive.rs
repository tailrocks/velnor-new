//! Bounded single-file analysis ZIP reader; extraction never touches disk.

use std::io::{Cursor, Read as _};

/// Analysis transport remains small even for large Cargo graphs.
pub(super) const MAX_ANALYSIS_BYTES: u64 = 4 * 1024 * 1024;
pub(super) const MAX_ARCHIVE_BYTES: usize = 8 * 1024 * 1024;

/// Accept exactly one ordinary, bounded, non-executable JSON payload.
pub(super) fn payload(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err("analysis_archive_oversize".to_owned());
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "analysis_archive_invalid".to_owned())?;
    if zip.len() != 1 {
        return Err("analysis_archive_shape".to_owned());
    }
    let file = zip
        .by_index(0)
        .map_err(|_| "analysis_archive_invalid".to_owned())?;
    let mode = file.unix_mode().unwrap_or(0o100644);
    if file.name() != "analysis.json"
        || file.is_dir()
        || mode & 0o170000 != 0o100000
        || mode & 0o111 != 0
        || file.size() > MAX_ANALYSIS_BYTES
        || file.size()
            > file
                .compressed_size()
                .saturating_mul(100)
                .saturating_add(1024)
    {
        return Err("analysis_archive_shape".to_owned());
    }
    let mut bytes = Vec::new();
    file.take(MAX_ANALYSIS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "analysis_archive_invalid".to_owned())?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_ANALYSIS_BYTES {
        return Err("analysis_archive_oversize".to_owned());
    }
    String::from_utf8(bytes).map_err(|_| "analysis_archive_invalid_utf8".to_owned())
}
