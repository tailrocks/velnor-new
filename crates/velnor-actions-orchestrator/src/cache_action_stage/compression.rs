use super::StageError;
use flate2::Compression;
use flate2::bufread::MultiGzDecoder;
use flate2::write::GzEncoder;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;
use velnor_actions_mise::{ZstdFilterMode, run_zstd_file_filter};

pub(crate) const MAX_ARCHIVE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const ZSTD_TIMEOUT: Duration = Duration::from_secs(300);

#[cfg(test)]
#[path = "compression_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchiveCompression {
    None,
    Gzip,
    Zstd,
    ZstdLong,
}

/// Cache SDK compression identity observed in the actual tar argument vector.
///
/// The names follow `@actions/cache` 6.1.0's `CompressionMethod` values. The
/// long-range zstd method remains distinct because it produces a different
/// cache version from zstd without long-range mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheActionCompression {
    /// SDK compression identity `gzip`.
    Gzip,
    /// SDK compression identity `zstd-without-long`.
    ZstdWithoutLong,
    /// SDK compression identity `zstd` (long-range mode).
    Zstd,
}

impl CacheActionCompression {
    /// Gives the exact pinned SDK method string used for cache versioning.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gzip => "gzip",
            Self::ZstdWithoutLong => "zstd-without-long",
            Self::Zstd => "zstd",
        }
    }
}

impl TryFrom<ArchiveCompression> for CacheActionCompression {
    type Error = StageError;

    fn try_from(value: ArchiveCompression) -> Result<Self, Self::Error> {
        match value {
            ArchiveCompression::Gzip => Ok(Self::Gzip),
            ArchiveCompression::Zstd => Ok(Self::ZstdWithoutLong),
            ArchiveCompression::ZstdLong => Ok(Self::Zstd),
            ArchiveCompression::None => Err("cache_tar_compression_missing"),
        }
    }
}

pub(crate) fn decode_archive(
    archive: File,
    compression: ArchiveCompression,
    zstd_path: Option<&Path>,
) -> Result<File, StageError> {
    let metadata = archive
        .metadata()
        .map_err(|_| "cache_tar_archive_open_failed")?;
    if !metadata.is_file() || metadata.len() > MAX_ARCHIVE_BYTES {
        return Err("cache_tar_archive_invalid");
    }
    let mut decoded = tempfile::tempfile().map_err(|_| "cache_tar_temporary_file_failed")?;
    match compression {
        ArchiveCompression::None => copy_bounded(archive, &mut decoded)?,
        ArchiveCompression::Gzip => {
            let mut reader = MultiGzDecoder::new(BufReader::new(archive));
            copy_bounded(&mut reader, &mut decoded).map_err(|_| "cache_tar_compression_failed")?;
            let mut source = reader.into_inner();
            if !source
                .fill_buf()
                .map_err(|_| "cache_tar_compression_failed")?
                .is_empty()
                || source
                    .stream_position()
                    .map_err(|_| "cache_tar_compression_failed")?
                    != metadata.len()
            {
                return Err("cache_tar_compression_trailing_data");
            }
        }
        ArchiveCompression::Zstd | ArchiveCompression::ZstdLong => {
            let program = checked_zstd(zstd_path)?;
            let mode = if compression == ArchiveCompression::ZstdLong {
                ZstdFilterMode::DecodeLong
            } else {
                ZstdFilterMode::Decode
            };
            run_zstd_file_filter(
                program,
                mode,
                archive,
                &mut decoded,
                MAX_ARCHIVE_BYTES,
                MAX_ARCHIVE_BYTES,
                ZSTD_TIMEOUT,
            )?;
        }
    }
    decoded
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cache_tar_temporary_file_failed")?;
    Ok(decoded)
}

pub(crate) fn encode_archive(
    mut raw: File,
    output: &mut File,
    compression: ArchiveCompression,
    zstd_path: Option<&Path>,
) -> Result<(), StageError> {
    let length = raw
        .metadata()
        .map_err(|_| "cache_tar_temporary_file_failed")?
        .len();
    if length > MAX_ARCHIVE_BYTES {
        return Err("cache_tar_archive_limit");
    }
    match compression {
        ArchiveCompression::None => copy_bounded(raw, output),
        ArchiveCompression::Gzip => {
            let mut encoder = GzEncoder::new(output, Compression::default());
            copy_bounded(raw, &mut encoder)?;
            encoder
                .finish()
                .map(|_| ())
                .map_err(|_| "cache_tar_archive_write_failed")
        }
        ArchiveCompression::Zstd | ArchiveCompression::ZstdLong => {
            let program = checked_zstd(zstd_path)?;
            raw.seek(SeekFrom::Start(0))
                .map_err(|_| "cache_tar_temporary_file_failed")?;
            let mode = if compression == ArchiveCompression::ZstdLong {
                ZstdFilterMode::EncodeLong
            } else {
                ZstdFilterMode::Encode
            };
            run_zstd_file_filter(
                program,
                mode,
                raw,
                output,
                MAX_ARCHIVE_BYTES,
                MAX_ARCHIVE_BYTES,
                ZSTD_TIMEOUT,
            )
        }
    }
}

fn checked_zstd(path: Option<&Path>) -> Result<&Path, StageError> {
    let path = path.ok_or("cache_tar_compression_unavailable")?;
    if !path.is_absolute() || path.as_os_str().len() > 4_096 {
        return Err("cache_tar_compression_unavailable");
    }
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "cache_tar_compression_unavailable")?;
    if !metadata.file_type().is_file() {
        return Err("cache_tar_compression_unavailable");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err("cache_tar_compression_unavailable");
        }
    }
    Ok(path)
}

fn copy_bounded<R: Read, W: Write>(mut reader: R, writer: &mut W) -> Result<(), StageError> {
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| "cache_tar_archive_read_failed")?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| "cache_tar_archive_limit")?)
            .ok_or("cache_tar_archive_limit")?;
        if total > MAX_ARCHIVE_BYTES {
            return Err("cache_tar_archive_limit");
        }
        writer
            .write_all(&buffer[..count])
            .map_err(|_| "cache_tar_archive_write_failed")?;
    }
    Ok(())
}
